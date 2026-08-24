#!/usr/bin/env python3
"""Create a complete, non-mutating production configuration plan."""
from __future__ import annotations
import argparse, datetime as dt, hashlib, json, os, re, sys
from pathlib import Path
from typing import Any
import yaml
from jinja2 import Environment, FileSystemLoader
sys.path.insert(0, str(Path(__file__).resolve().parent))
from pve_discover import PVEClient  # noqa: E402
from guest_plan import lxc_plan, qemu_plan, parse_options  # noqa: E402

def load(p: Path): return yaml.safe_load(p.read_text())
def opt(parts): return ",".join(f"{k}={v}" for k,v in parts if v is not None)
def lxc_payload(d):
    n=d["network"]
    payload={"hostname":d["hostname"],"cores":str(d["cores"]),"memory":str(d["memory_mb"]),"swap":str(d["swap_mb"]),"onboot":"1" if d["start"]["onboot"] else "0","startup":opt([("order",d["start"]["order"]),("up",d["start"].get("delay_seconds"))]),"net0":opt([("name",n["name"]),("bridge",n["bridge"]),("firewall",1 if n.get("firewall") else 0),("gw",n.get("gateway4")),("gw6",n.get("gateway6")),("hwaddr",n["mac"]),("ip",n["ipv4"]),("ip6",n.get("ipv6")),("type","veth")])}
    for i,n in enumerate(d.get("additional_networks",[]),1): payload[f"net{i}"]=opt([("name",n["name"]),("bridge",n["bridge"]),("firewall",1 if n.get("firewall") else 0),("hwaddr",n["mac"]),("ip",n["ipv4"]),("type","veth")])
    return payload
def vm_payload(d):
    p={"name":d["name"],"machine":d["machine"],"bios":d["bios"],"cores":str(d["cpu"]["cores"]),"sockets":str(d["cpu"]["sockets"]),"memory":str(d["memory_mb"]),"cpu":d["cpu"]["type"],"onboot":"1" if d["start"]["onboot"] else "0","agent":"1" if d["qemu_guest_agent"] else "0","startup":opt([("order",d["start"]["order"])])}
    for i,n in enumerate(d["networks"]): p[f"net{i}"]=opt([(n["model"],n["mac"]),("bridge",n["bridge"]),("firewall",1 if n.get("firewall") else 0),("tag",n.get("vlan"))])
    for u in d.get("usb_passthrough",[]): p[u["slot"]]=f"host={u['host']}"
    return p
def changed_payload(payload, actual):
    out={}
    for k,v in payload.items():
        av=str(actual.get(k,""))
        if k.startswith("net"):
            wanted=parse_options(v); have=parse_options(av)
            if any(str(have.get(x,"0" if x=="firewall" else "")) != str(y) for x,y in wanted.items()): out[k]=v
        elif k.startswith("usb"):
            if parse_options(av).get("host") != parse_options(v).get("host"): out[k]=v
        elif k == "agent" and str(actual.get(k, "0")) == str(v): pass
        elif av != str(v): out[k]=v
    return out
def semantic(s): return [x.strip() for x in s.splitlines() if x.strip() and not x.lstrip().startswith('#')]
def firewall_semantic(s):
    section=''; options=[]; rules=[]
    for line in semantic(s):
      if line.startswith('['): section=line
      elif section=='[OPTIONS]': options.append(line)
      elif section=='[RULES]': rules.append(line)
    return sorted(options), rules
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--root',type=Path,required=True); ap.add_argument('--output',type=Path,required=True); a=ap.parse_args(); root=a.root
    g=load(root/'config/guests.yml'); fw=load(root/'config/firewall.yml'); net=load(root/'config/network.yml'); node=g['node']
    manifest=load(root/'exports/production/manifest.json')
    export_age=dt.datetime.now(dt.timezone.utc)-dt.datetime.fromisoformat(manifest['exported_at'])
    if export_age > dt.timedelta(minutes=30): raise SystemExit('sanitized exports are stale; run ./bin/iac capture before planning')
    c=PVEClient(os.environ['PVE_ENDPOINT'],os.environ['PVE_API_TOKEN_ID'],os.environ['PVE_API_TOKEN_SECRET']); ops=[]; blockers=[]
    for kind,key,fn,builder in [('lxc','lxcs',lxc_plan,lxc_payload),('qemu','vms',qemu_plan,vm_payload)]:
      for vmid,d in g.get(key,{}).items():
        actual=c.get(f'/nodes/{node}/{kind}/{vmid}/config'); drift=fn(str(vmid),d,actual)
        payload=changed_payload(builder(d),actual)
        if payload: ops.append({'domain':'guests','resource':f'{kind}/{vmid}','action':'update','endpoint':f'/nodes/{node}/{kind}/{vmid}/config','changes':payload,'before':actual,'drift':drift})
        rootfs=parse_options(actual.get('rootfs' if kind=='lxc' else 'scsi0'))
        desired_disk=d['rootfs' if kind=='lxc' else 'disk']; actual_store=rootfs.get('volume','').split(':')[0]
        if actual_store != desired_disk['storage']: blockers.append(f'{kind}/{vmid}: storage moves are not automatically applied')
        size=int(rootfs.get('size','0G').removesuffix('G')); wanted=int(desired_disk['size_gb'])
        if wanted < size: blockers.append(f'{kind}/{vmid}: disk shrinking is forbidden')
        elif wanted > size: ops.append({'domain':'guests','resource':f'{kind}/{vmid}/disk','action':'grow-disk','disk':'rootfs' if kind=='lxc' else d['disk']['interface'],'size_gb':wanted})
    env=Environment(loader=FileSystemLoader(root/'templates'))
    rendered=env.get_template('cluster.fw.j2').render(firewall=fw)
    current=(root/'exports/production/pve/firewall/cluster.fw').read_text()
    if firewall_semantic(rendered)!=firewall_semantic(current): ops.append({'domain':'firewall','resource':'cluster','action':'write-file','path':'/etc/pve/firewall/cluster.fw','content':rendered})
    for vmid,p in fw['guests'].items():
      rendered=env.get_template('guest.fw.j2').render(guest_firewall=p); path=root/f'exports/production/pve/firewall/{vmid}.fw'; current=path.read_text() if path.exists() else ''
      if firewall_semantic(rendered)!=firewall_semantic(current): ops.append({'domain':'firewall','resource':str(vmid),'action':'write-file','path':f'/etc/pve/firewall/{vmid}.fw','content':rendered})
    rendered=env.get_template('network-interfaces.j2').render(network=net); current=(root/'exports/production/network/interfaces').read_text()
    if semantic(rendered)!=semantic(current): ops.append({'domain':'network','resource':node,'action':'write-network','path':'/etc/network/interfaces','content':rendered})
    dns=net.get('dns',{}); actual_dns=c.get(f'/nodes/{node}/dns'); desired_dns={**({'search':dns['search']} if dns.get('search') else {}),**{f'dns{i+1}':v for i,v in enumerate(dns.get('servers',[]))}}
    dns_changes={k:str(v) for k,v in desired_dns.items() if str(actual_dns.get(k,''))!=str(v)}
    if dns_changes: ops.append({'domain':'dns','resource':node,'action':'api-update','endpoint':f'/nodes/{node}/dns','changes':dns_changes})
    body={'schema_version':1,'created_at':dt.datetime.now(dt.timezone.utc).isoformat(),'target':os.environ['PVE_ENDPOINT'],'operations':ops,'blockers':blockers}
    body['plan_sha256']=hashlib.sha256(json.dumps(body,sort_keys=True,separators=(',',':')).encode()).hexdigest(); a.output.parent.mkdir(parents=True,exist_ok=True); a.output.write_text(json.dumps(body,indent=2)+'\n'); print(json.dumps({**body,'operations':ops},indent=2)); return 2 if blockers else 0
if __name__=='__main__': raise SystemExit(main())
