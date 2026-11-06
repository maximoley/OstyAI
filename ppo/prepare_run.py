"""Create a fresh, self-contained PPO run without touching any existing training."""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import shutil
import sys

HERE = Path(__file__).resolve().parent


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def prepare(character, output, library=None, anneal_start=None, stop_at=None):
    if bool(anneal_start) != bool(stop_at):
        raise ValueError('Supply both --anneal-start and --stop-at to shift the historical schedule')
    config = json.loads((HERE / 'configs' / f'{character.lower()}.json').read_text())
    if anneal_start:
        sys.path.insert(0, str(HERE / 'trainer'))
        from training_schedule import timestamp
        if timestamp(anneal_start) >= timestamp(stop_at):
            raise ValueError('Anneal start must precede stop time')
        config['schedule'].update(adapt_lr_until=anneal_start, anneal_start=anneal_start,
                                  anneal_end=stop_at, stop_at=stop_at)
    output = Path(output).resolve()
    if output.exists():
        raise FileExistsError(f'Refusing to overwrite an existing directory: {output}')
    if library is not None:
        library = Path(library).resolve()
        lib = ctypes.CDLL(str(library))
        lib.sm_fr_character.restype = ctypes.c_char_p
        actual = lib.sm_fr_character().decode()
        if actual != character:
            raise ValueError(f'Native character {actual} does not match requested {character}')
    original = json.loads((HERE / 'SOURCE.json').read_text())
    schema = HERE.parent / 'ostyai/schema'
    for name, expected in original['schema_sha256'].items():
        if sha(schema / name) != expected:
            raise ValueError(f'Bundled schema mismatch: {name}')
    if sha(HERE / 'validation-seeds.json') != original['validation_seed_sha256']:
        raise ValueError('Validation seed list differs from the training runs')
    output.mkdir(parents=True)
    shutil.copytree(HERE / 'trainer', output, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns('__pycache__', '.pytest_cache', '._*'))
    shutil.copytree(schema, output / 'schema')
    shutil.copy2(HERE / 'validation-seeds.json', output / 'validation-seeds.json')
    (output / 'config.json').write_text(json.dumps(config, indent=2) + '\n')
    if library:
        shutil.copy2(library, output / 'loop_native.so')
    receipt = dict(character=character, fresh=True, checkpoint_imported=False,
                   native_sha256=sha(library) if library else None,
                   config_sha256=sha(output / 'config.json'),
                   source_snapshot_sha256=sha(HERE / 'SOURCE.json'),
                   explicit_schedule_override=bool(anneal_start))
    (output / 'package.json').write_text(json.dumps(receipt, indent=2) + '\n')
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--character', choices=['Necrobinder', 'Ironclad'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--native-library', type=Path,
                        help='Omit only for CPU tests; training needs the character-specific library')
    parser.add_argument('--anneal-start', help='ISO timestamp with timezone offset')
    parser.add_argument('--stop-at', help='ISO timestamp with timezone offset')
    args = parser.parse_args()
    print(prepare(args.character, args.output, args.native_library, args.anneal_start, args.stop_at))


if __name__ == '__main__':
    main()
