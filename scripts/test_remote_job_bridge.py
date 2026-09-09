import importlib.util
import json
import pathlib
import subprocess
import tempfile
import types
import unittest
from unittest import mock
import uuid

spec = importlib.util.spec_from_file_location('remote_bridge', pathlib.Path(__file__).resolve().parents[1] / 'src-tauri/src/remote_job_bridge.py')
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)


class RemoteJobBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name).resolve()
        self.identity = dict(job_id=str(uuid.uuid4()), request_sha256='a' * 64,
                             remote_root=str(self.root), context=dict(project_id=str(uuid.uuid4()), run_id=str(uuid.uuid4()), backend_id='ssh:fake', language='python', environment='system'))
        self.payload = dict(action='submit', identity=self.identity, code='print(42)', captures=['answer.txt'])

    def test_submission_detaches_and_repeated_submission_only_observes(self):
        with mock.patch.object(subprocess, 'Popen') as launch:
            self.assertEqual(bridge.submit(self.payload)['status'], 'submitted')
            launch.assert_called_once()
            kwargs = launch.call_args.kwargs
            self.assertTrue(kwargs['start_new_session'])
            self.assertEqual(kwargs['stdin'], subprocess.DEVNULL)
            self.assertEqual(kwargs['stdout'], subprocess.DEVNULL)
            self.assertEqual(bridge.submit(self.payload)['status'], 'unknown')
            launch.assert_called_once()
        changed = dict(self.identity, request_sha256='b' * 64)
        with self.assertRaises(ValueError):
            bridge.status(changed)

    def test_partial_submission_and_dead_worker_never_launch_again(self):
        directory = bridge.job_directory(self.identity, create=True)
        directory.mkdir()
        with mock.patch.object(subprocess, 'Popen') as launch:
            self.assertEqual(bridge.submit(self.payload)['status'], 'unknown')
            launch.assert_not_called()
        (directory / 'request.json').write_text(json.dumps(dict(identity=self.identity)))
        (directory / 'process.json').write_text(json.dumps(dict(pid=999999999, birth='old')))
        self.assertEqual(bridge.status(self.identity)['status'], 'unknown')

    def test_worker_result_can_be_read_by_new_controller_without_running_code(self):
        with mock.patch.object(subprocess, 'Popen'):
            bridge.submit(self.payload)
        directory = bridge.job_directory(self.identity)
        namespace = {'__name__': 'test_worker'}
        exec(bridge.WORKER, namespace)
        original_read = pathlib.Path.read_text
        def read(path, *args, **kwargs):
            if str(path).replace('\\', '/').startswith('/proc/'):
                return '123 (test) S ' + '0 ' * 18 + 'birth'
            return original_read(path, *args, **kwargs)
        def execute(argv, **kwargs):
            self.assertEqual(kwargs['cwd'], self.root)
            self.assertEqual(kwargs['stdin'], subprocess.DEVNULL)
            kwargs['stdout'].write(b'x' * 20000)
            (self.root / 'answer.txt').write_text('42')
            return types.SimpleNamespace(returncode=0)
        with mock.patch.object(pathlib.Path, 'read_text', read), mock.patch.object(subprocess, 'run', side_effect=execute) as run:
            namespace['worker'](str(directory))
            run.assert_called_once()
        new_spec = importlib.util.spec_from_file_location('reconnected_bridge', spec.origin)
        fresh = importlib.util.module_from_spec(new_spec)
        new_spec.loader.exec_module(fresh)
        with mock.patch.object(subprocess, 'Popen') as launch, mock.patch.object(subprocess, 'run') as run:
            result = fresh.status(self.identity)
            launch.assert_not_called()
            run.assert_not_called()
        self.assertEqual(result['status'], 'completed')
        self.assertTrue(result['result']['succeeded'])
        self.assertEqual(result['result']['request_id'], self.identity['job_id'])
        self.assertTrue(result['result']['stdout_capture']['truncated'])
        self.assertEqual(result['result']['stdout_capture']['total_bytes'], 20000)
        self.assertEqual(len(result['result']['stdout']), 16384)
        self.assertEqual(result['result']['artifacts'][0]['relative_path'], 'answer.txt')
        self.assertFalse((directory / 'result.tmp').exists())

    def test_rejects_receipt_identity_tampering_and_oversized_receipts(self):
        with mock.patch.object(subprocess, 'Popen'):
            bridge.submit(self.payload)
        result = bridge.job_directory(self.identity) / 'result.json'
        result.write_text(json.dumps(dict(identity={}, status='completed')))
        with self.assertRaises(ValueError):
            bridge.status(self.identity)
        result.write_text('x' * (512 * 1024 + 1))
        with self.assertRaises(ValueError):
            bridge.status(self.identity)

    def test_rejects_control_directory_escape_before_creating_directories(self):
        original = pathlib.Path.resolve
        def resolve(path, *args, **kwargs):
            if path.name == 'remote-jobs':
                return self.root.parent / 'outside-test-sentinel'
            return original(path, *args, **kwargs)
        with mock.patch.object(pathlib.Path, 'resolve', resolve), self.assertRaises(ValueError):
            bridge.job_directory(self.identity, create=True)
        self.assertFalse((self.root / '.omicsops').exists())
