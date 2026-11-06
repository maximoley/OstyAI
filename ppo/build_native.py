"""Build an isolated character-specific PPO library from the bundled Rust engine."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE / 'trainer'))
from cuda_runtime import runtime_library_path


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def character_source(text, character):
    if character not in {'Necrobinder', 'Ironclad'}:
        raise ValueError('Only the two published character builds are supported')
    for old, new in [
        ('CharacterId::Necrobinder;', f'CharacterId::{character};'),
        ('CharacterId::Necrobinder => b"Necrobinder\\0"',
         f'CharacterId::{character} => b"{character}\\0"'),
    ]:
        if text.count(old) != 1:
            raise ValueError(f'Unexpected character template: {old}')
        text = text.replace(old, new)
    return text


def assemble(character, build_dir):
    base = json.loads((HERE / 'native-base.json').read_text())
    for name, expected in base['files'].items():
        path = REPO / 'native' / name
        if not path.is_file() or sha(path) != expected:
            raise RuntimeError(f'Published simulator base differs: {name}')
    overlay = {str(p.relative_to(HERE / 'native')): sha(p)
               for p in (HERE / 'native').rglob('*.rs')}
    receipt = dict(character=character, base_manifest_sha256=sha(HERE / 'native-base.json'),
                   overlay_sha256=overlay)
    build_dir = Path(build_dir).resolve()
    source = build_dir / 'native'
    stamp = build_dir / 'assembly.json'
    if source.exists():
        if not stamp.exists() or json.loads(stamp.read_text()) != receipt:
            raise RuntimeError('Build directory belongs to different inputs; choose a fresh --build-dir')
    else:
        build_dir.mkdir(parents=True, exist_ok=True)
        shutil.copytree(REPO / 'native', source,
                        ignore=shutil.ignore_patterns('target', '.git', '.hegel', '__pycache__', '._*'))
        module = source / 'crates/sts2-py/src/adversarial.rs'
        text = module.read_text()
        if 'pub mod fullrun;' in text:
            raise RuntimeError('Base already contains a full-run module; refusing an ambiguous overlay')
        module.write_text(text + '\npub mod fullrun;\n')
        destination = module.parent / 'adversarial'
        for name in overlay:
            output = destination / name
            output.parent.mkdir(parents=True, exist_ok=True)
            text = (HERE / 'native' / name).read_text()
            output.write_text(character_source(text, character) if name == 'fullrun.rs' else text)
        stamp.write_text(json.dumps(receipt, indent=2) + '\n')
    # Recheck cached source too: never silently compile a locally modified assembly.
    for name in base['files']:
        if name == 'crates/sts2-py/src/adversarial.rs':
            expected = (REPO / 'native' / name).read_text() + '\npub mod fullrun;\n'
            if (source / name).read_text() != expected:
                raise RuntimeError(f'Assembled source differs: {name}')
        elif sha(source / name) != base['files'][name]:
            raise RuntimeError(f'Assembled source differs: {name}')
    for name in overlay:
        expected = (HERE / 'native' / name).read_text()
        if name == 'fullrun.rs':
            expected = character_source(expected, character)
        if (source / 'crates/sts2-py/src/adversarial' / name).read_text() != expected:
            raise RuntimeError(f'Assembled overlay differs: {name}')
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--character', choices=['Necrobinder', 'Ironclad'], required=True)
    parser.add_argument('--build-dir', type=Path)
    parser.add_argument('--jobs', type=int, default=2)
    parser.add_argument('--assemble-only', action='store_true')
    args = parser.parse_args()
    build_dir = (args.build_dir or HERE / 'build' / args.character.lower()).resolve()
    source = assemble(args.character, build_dir)
    if args.assemble_only:
        print(source)
        return
    env = dict(os.environ, PYO3_PYTHON=sys.executable,
               CARGO_TARGET_DIR=str(build_dir / 'target'))
    # CUDA wheel installations often lack the development linker alias.
    library = Path(runtime_library_path())
    if not library.is_file():
        raise RuntimeError('Set CUDA_RUNTIME_LIBRARY to the full libcudart path for linking')
    link_dir = build_dir / 'cuda-link'
    link_dir.mkdir(exist_ok=True)
    for name in {library.name, 'libcudart.so'}:
        link = link_dir / name
        if link.is_symlink() or link.exists():
            if link.resolve() != library.resolve():
                raise RuntimeError(f'CUDA link points at another runtime: {link}')
        else:
            link.symlink_to(library.resolve())
    env['CUDA_LIB_DIR'] = str(link_dir)
    command = ['cargo', 'build', '--release', '--locked', '-j', str(args.jobs),
               '-p', 'sts2-py', '--lib', '--features', 'native-gpu-feeder,pyo3/extension-module']
    subprocess.run(command, cwd=source, env=env, check=True)
    output = build_dir / 'loop_native.so'
    shutil.copy2(build_dir / 'target/release/libsts2_rs.so', output)
    receipt = dict(character=args.character, native_sha256=sha(output),
                   command=command, base_manifest_sha256=sha(HERE / 'native-base.json'),
                   features=['native-gpu-feeder', 'pyo3/extension-module'])
    (build_dir / 'build.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(output)


if __name__ == '__main__':
    main()
