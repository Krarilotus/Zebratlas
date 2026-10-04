"""Verify and run the small public sample; mutable state stays temporary."""
import argparse, hashlib, json, os, stat, subprocess, tempfile
from pathlib import Path

def no_link(path):
    if path.is_symlink() or getattr(path.lstat(),'st_file_attributes',0)&stat.FILE_ATTRIBUTE_REPARSE_POINT:
        raise ValueError('Sample paths must be ordinary files/directories')

def verify(root):
    no_link(root)
    root=root.resolve()
    manifest=json.loads((root/'sample-manifest.json').read_text(encoding='utf-8'))
    if manifest.get('format')!='zebratlas-public-sample-manifest-v1' or manifest.get('status')!='verified_local_sample':
        raise ValueError('Sample has no verified manifest')
    expected={x['file']:x for x in manifest['files']}
    actual=set()
    for directory,folders,files in os.walk(root,followlinks=False):
        for name in folders+files:no_link(Path(directory)/name)
        for name in files:actual.add((Path(directory)/name).relative_to(root).as_posix())
    actual.discard('sample-manifest.json')
    if actual!=set(expected):raise ValueError('Unexpected or missing sample files')
    for name,item in expected.items():
        original=root/name;no_link(original);path=original.resolve()
        if not path.is_relative_to(root):raise ValueError('Unsafe sample path')
        if path.stat().st_size!=item['bytes'] or hashlib.sha256(path.read_bytes()).hexdigest()!=item['sha256']:
            raise ValueError('Sample checksum mismatch: '+name)
    return manifest

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,default=Path('target/release/atlas-server'))
    parser.add_argument('--sample',type=Path,default=Path('data/public-sample'))
    parser.add_argument('--port',type=int,default=8000)
    args=parser.parse_args()
    no_link(args.sample)
    root=args.sample.resolve();binary=args.binary.resolve()
    if os.name=='nt' and not binary.exists():binary=binary.with_suffix('.exe')
    if not binary.is_file() or not 1024<=args.port<=65535:raise ValueError('Invalid binary or port')
    verify(root)
    forbidden=['ATLAS_OPERATIONAL_MANIFEST','RARE_ATLAS_REQUIRE_IDENTITY_GATE','RARE_ATLAS_MAPPING_DIR',
        'RARE_ATLAS_IDENTITY_MANIFEST_SHA256','RARE_ATLAS_ATLAS_SNAPSHOT','RARE_ATLAS_GRAPH_SNAPSHOT',
        'RARE_ATLAS_MECHANISM_SNAPSHOT','RARE_ATLAS_SNAPSHOT','RARE_ATLAS_SNAPSHOTS']
    if any(os.environ.get(key) for key in forbidden):raise ValueError('Use a separate shell from private production settings')
    env=os.environ.copy()
    for key in list(env):
        if any(word in key.upper() for word in ['API_KEY','API_TOKEN','PASSWORD','SECRET']):env.pop(key)
    env.update(RARE_ATLAS_DATA=str(root),RARE_ATLAS_SNAPSHOTS=str(root/'cache'),
        RARE_ATLAS_ACCOUNTS_DB=':memory:',RARE_ATLAS_CONTRIB_DB=':memory:')
    with tempfile.TemporaryDirectory(prefix='zebratlas-public-sample-') as state:
        print(f'Small public sample API: http://127.0.0.1:{args.port}. Press Ctrl+C to stop.',flush=True)
        process=subprocess.Popen([str(binary),'--data',str(root),'serve','--read-only-snapshots',
            '--addr',f'127.0.0.1:{args.port}'],env=env,cwd=state)
        interrupted=False
        try:process.wait()
        except KeyboardInterrupt:
            interrupted=True
            process.terminate()
            try:process.wait(timeout=10)
            except subprocess.TimeoutExpired:process.kill();process.wait()
        finally:verify(root)
        if not interrupted and process.returncode:raise SystemExit(process.returncode)

if __name__=='__main__':main()
