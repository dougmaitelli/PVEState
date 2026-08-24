#!/usr/bin/env python3
"""Apply an explicitly confirmed, fresh, non-destructive production plan."""
from __future__ import annotations
import argparse, base64, datetime as dt, hashlib, json, os, shlex, subprocess
from pathlib import Path
from pve_discover import PVEClient

ALLOWED={"guests","firewall","network","dns"}
def required(n):
    v=os.environ.get(n)
    if not v: raise SystemExit(f'missing required environment variable: {n}')
    return v
def ssh_base():
    return ['ssh','-p',os.getenv('IAC_APPLY_SSH_PORT','22'),'-o','BatchMode=yes','-o','IdentitiesOnly=yes','-o','StrictHostKeyChecking=yes','-o',f"UserKnownHostsFile={required('IAC_APPLY_KNOWN_HOSTS')}",'-i',required('IAC_APPLY_SSH_KEY'),f"{os.getenv('IAC_APPLY_SSH_USER','root')}@{required('IAC_APPLY_SSH_HOST')}"]
def ssh(command, stdin=None, capture=False): return subprocess.run(ssh_base()+[command],input=stdin,text=True,check=True,timeout=90,capture_output=capture)
def write_file(path, content, expected_sha, stamp):
    current=ssh(f"sha256sum {shlex.quote(path)}",capture=True).stdout.split()[0]
    if current != expected_sha: raise SystemExit(f'remote file changed after plan: {path}')
    encoded=base64.b64encode(content.encode()).decode(); backup=f'/root/iac-preapply/{stamp}{path}'
    mode='0644' if path=='/etc/network/interfaces' else '0640'
    cmd=f"install -d {shlex.quote(str(Path(backup).parent))} && (test ! -e {shlex.quote(path)} || cp -a {shlex.quote(path)} {shlex.quote(backup)}) && base64 -d > {shlex.quote(path+'.iac-new')} && install -m {mode} {shlex.quote(path+'.iac-new')} {shlex.quote(path)} && rm {shlex.quote(path+'.iac-new')}"
    ssh(cmd, encoded)
def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--plan',type=Path,required=True); a=ap.parse_args(); plan=json.loads(a.plan.read_text())
    claimed_hash=plan['plan_sha256']; unsigned={k:v for k,v in plan.items() if k!='plan_sha256'}
    actual_hash=hashlib.sha256(json.dumps(unsigned,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    if claimed_hash != actual_hash: raise SystemExit('plan file integrity check failed')
    if required('IAC_ENABLE_PRODUCTION_APPLY')!='YES': raise SystemExit('IAC_ENABLE_PRODUCTION_APPLY must equal YES')
    if required('IAC_CONFIRM_PLAN_SHA')!=plan['plan_sha256']: raise SystemExit('plan SHA confirmation mismatch')
    if required('IAC_APPLY_TARGET')!=plan['target']: raise SystemExit('apply target does not match plan target')
    if plan['blockers']: raise SystemExit(f"plan has blockers: {plan['blockers']}")
    age=dt.datetime.now(dt.timezone.utc)-dt.datetime.fromisoformat(plan['created_at'])
    if age>dt.timedelta(minutes=30): raise SystemExit('plan is stale; run capture and plan again')
    allowed=set(filter(None,required('IAC_APPLY_DOMAINS').split(',')))
    if not allowed<=ALLOWED: raise SystemExit(f'unknown apply domain: {allowed-ALLOWED}')
    unapproved={x['domain'] for x in plan['operations']}-allowed
    if unapproved: raise SystemExit(f'plan contains unapproved domains: {sorted(unapproved)}')
    client=PVEClient(required('PVE_APPLY_ENDPOINT'),required('PVE_APPLY_API_TOKEN_ID'),required('PVE_APPLY_API_TOKEN_SECRET'))
    if client.base_url.removesuffix('/api2/json') != plan['target'].rstrip('/'): raise SystemExit('mutation API endpoint differs from planned endpoint')
    stamp=dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    results=[]
    for op in plan['operations']:
      if op['action'] in {'update','api-update'}: client.put(op['endpoint'],op['changes'])
      elif op['action']=='grow-disk':
        kind,vmid,_=op['resource'].split('/'); client.put(f"/nodes/pve/{kind}/{vmid}/resize",{'disk':op['disk'],'size':f"{op['size_gb']}G"})
      elif op['action']=='write-file': write_file(op['path'],op['content'],op['before_sha256'],stamp)
      elif op['action']=='write-network':
        write_file(op['path'],op['content'],op['before_sha256'],stamp)
        if os.getenv('IAC_APPLY_NETWORK_NOW')=='YES': ssh('ifreload -a')
        else: results.append({'resource':op['resource'],'status':'written-not-activated'}); continue
      else: raise SystemExit(f"unsupported action: {op['action']}")
      results.append({'resource':op['resource'],'status':'applied'})
    out=a.plan.parent/f'apply-{stamp}.json'; out.write_text(json.dumps({'plan_sha256':plan['plan_sha256'],'results':results},indent=2)+'\n'); print(json.dumps(results,indent=2))
if __name__=='__main__': main()
