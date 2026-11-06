"""CPU-only distribution checks, independent of training or a CUDA device."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Packaging(unittest.TestCase):
    def test_prepare_both_recipes_and_frozen_seeds(self):
        prepare = load('prepare_run')
        for character in ['Ironclad', 'Necrobinder']:
            with self.subTest(character=character), tempfile.TemporaryDirectory() as directory:
                run = prepare.prepare(character, Path(directory) / 'run')
                config = json.loads((run / 'config.json').read_text())
                original = json.loads((HERE / 'configs' / f'{character.lower()}.json').read_text())
                self.assertEqual(config, original)
                self.assertEqual((run / 'schema/numeric_catalog.json').read_bytes(),
                                 (HERE.parent / 'ostyai/schema/numeric_catalog.json').read_bytes())
                seeds = json.loads((run / 'validation-seeds.json').read_text())
                self.assertEqual(len(set(seeds)), 1024)
                self.assertTrue(all(2**63 <= x < 2**64 for x in seeds))
                self.assertFalse((run / 'production').exists())
                with self.assertRaises(FileExistsError):
                    prepare.prepare(character, run)

    def test_explicit_schedule_override(self):
        prepare = load('prepare_run')
        with tempfile.TemporaryDirectory() as directory:
            run = prepare.prepare('Necrobinder', Path(directory) / 'run',
                                  anneal_start='2027-01-01T06:00:00-05:00',
                                  stop_at='2027-01-02T06:00:00-05:00')
            config = json.loads((run / 'config.json').read_text())
            self.assertEqual(config['schedule']['adapt_lr_until'], config['schedule']['anneal_start'])
            self.assertEqual(config['schedule']['anneal_end'], config['schedule']['stop_at'])
            with self.assertRaises(ValueError):
                prepare.prepare('Necrobinder', Path(directory) / 'other', stop_at='2027-01-02T06:00:00-05:00')

    def test_character_template_matches_recorded_factories(self):
        import hashlib
        build = load('build_native')
        source = (HERE / 'native/fullrun.rs').read_text()
        recorded = json.loads((HERE / 'SOURCE.json').read_text())['original_character_factory_sha256']
        for character in ['Necrobinder', 'Ironclad']:
            rendered = build.character_source(source, character)
            self.assertIn(f'const RUN_CHARACTER: sts2_core::CharacterId = sts2_core::CharacterId::{character};', rendered)
            self.assertIn(f'=> b"{character}\\0"', rendered)
            self.assertEqual(hashlib.sha256(rendered.encode()).hexdigest(), recorded[character])

    def test_math_and_model_sources_preserved(self):
        import hashlib
        source = json.loads((HERE / 'SOURCE.json').read_text())['python']
        for name in ['model.py', 'learner.py', 'returns.py', 'filtering.py', 'collect_fast.py',
                     'rollout.py', 'fast_workers.py', 'train_production.py', 'validation.py',
                     'training_schedule.py', 'supervise_run.py']:
            self.assertEqual(hashlib.sha256((HERE / 'trainer' / name).read_bytes()).hexdigest(), source[name])


if __name__ == '__main__':
    unittest.main()
