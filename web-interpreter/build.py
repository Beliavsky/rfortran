"""Build the real Rust interpreter for the browser; no native compiler backend."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--debug',action='store_true',help='Faster development build; larger download')
    args=parser.parse_args()
    commit=subprocess.run(['git','rev-parse','--short','HEAD'],cwd=ROOT,capture_output=True,text=True).stdout.strip() or 'development'
    if subprocess.run(['git','diff','--quiet'],cwd=ROOT,stderr=subprocess.DEVNULL).returncode:
        commit+='-dirty'
    command=['cargo','build','--locked','--manifest-path',str(ROOT/'web-interpreter/wasm/Cargo.toml'),
             '--target','wasm32-wasip1','--target-dir',str(ROOT/'target/browser-wasm')]
    if not args.debug: command.append('--release')
    env={**os.environ,'RFORT_COMMIT':commit,
         'CARGO_TARGET_WASM32_WASIP1_RUSTFLAGS':'-C link-arg=-zstack-size=8388608 -C link-arg=--max-memory=268435456'}
    result=subprocess.run(command,cwd=ROOT,env=env)
    if result.returncode:
        print('If the target is missing, run: rustup target add wasm32-wasip1',file=sys.stderr)
        return result.returncode
    source=ROOT/'target/browser-wasm/wasm32-wasip1'/('debug' if args.debug else 'release')/'rfort-browser.wasm'
    output=ROOT/'web-interpreter/runtime'
    output.mkdir(exist_ok=True)
    shutil.copyfile(source,output/source.name)
    shutil.copyfile(ROOT/'LICENSE',output/'LICENSE-GPLv3.txt')
    shutil.copyfile(ROOT/'LICENSE-MIT',output/'LICENSE-MIT.txt')
    revision=subprocess.run(['git','rev-parse','HEAD'],cwd=ROOT,capture_output=True,text=True).stdout.strip()
    info={'commit':commit,'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'bytes':source.stat().st_size,
          'source':f'https://github.com/Beliavsky/rfortran/tree/{revision}'}
    (output/'build.json').write_text(json.dumps(info,indent=2)+'\n',encoding='utf-8')
    print(f'Built {output/source.name} ({source.stat().st_size:,} bytes)')
    print('Serve: python -m http.server 8000 --bind 127.0.0.1 --directory web-interpreter')
    return 0

if __name__=='__main__': sys.exit(main())
