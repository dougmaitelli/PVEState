#!/usr/bin/env python3
"""Validate cross-file invariants in the custom h4des IaC model."""
from pathlib import Path
import argparse, ipaddress, re, yaml
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--config-dir',type=Path,required=True); a=ap.parse_args()
    load=lambda n: yaml.safe_load((a.config_dir/n).read_text())
    g,fw,net=load('guests.yml'),load('firewall.yml'),load('network.yml'); errors=[]
    vmids={int(x) for x in g.get('lxcs',{})}|{int(x) for x in g.get('vms',{})}
    if set(map(int,fw['guests']))-vmids: errors.append('firewall policy references an unmanaged VMID')
    bridges={x['name'] for x in net['bridges']}
    macs=set()
    for vmid,d in g.get('lxcs',{}).items():
      nics=[d['network'],*d.get('additional_networks',[])]
      names=set()
      for n in nics:
        if n['bridge'] not in bridges: errors.append(f'lxc/{vmid}: unknown bridge {n["bridge"]}')
        try: ipaddress.ip_interface(n['ipv4'])
        except ValueError: errors.append(f'lxc/{vmid}: invalid IPv4 {n["ipv4"]}')
        if n['name'] in names: errors.append(f'lxc/{vmid}: duplicate NIC name {n["name"]}')
        names.add(n['name']); mac=n['mac'].upper()
        if not re.fullmatch(r'(?:[0-9A-F]{2}:){5}[0-9A-F]{2}',mac): errors.append(f'lxc/{vmid}: invalid MAC {mac}')
        if mac in macs: errors.append(f'duplicate MAC {mac}')
        macs.add(mac)
    for vmid,d in g.get('vms',{}).items():
      for n in d['networks']:
        if n['bridge'] not in bridges: errors.append(f'qemu/{vmid}: unknown bridge {n["bridge"]}')
        if n['mac'].upper() in macs: errors.append(f'duplicate MAC {n["mac"]}')
        macs.add(n['mac'].upper())
      slots=[u['slot'] for u in d.get('usb_passthrough',[])]
      if len(slots)!=len(set(slots)): errors.append(f'qemu/{vmid}: duplicate USB slot')
    if errors: raise SystemExit('\n'.join(f'error: {x}' for x in errors))
    print(f'configuration valid: {len(vmids)} guests, {len(fw["guests"])} guest firewalls, {len(macs)} NICs')
if __name__=='__main__': main()
