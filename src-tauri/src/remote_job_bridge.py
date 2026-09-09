"""Linux SSH one-shot jobs. Controller invocations never wait for computation."""
import base64
import json
import os
import pathlib
import subprocess
import sys

WORKER = r"""
import hashlib
import json
import os
import pathlib
import subprocess
import sys


def atomic(path, value):
    tmp = path.with_suffix('.tmp')
    with tmp.open('w', encoding='utf-8') as stream:
        json.dump(value, stream, ensure_ascii=False)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(tmp, path)


def capture(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        excerpt = stream.read(16384)
        digest.update(excerpt)
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    size = path.stat().st_size
    return dict(excerpt=excerpt.decode('utf-8', errors='replace'), total_bytes=size,
                sha256=digest.hexdigest(), archive_path=str(path), truncated=size > len(excerpt))


def worker(directory):
    directory = pathlib.Path(directory)
    request = json.loads((directory / 'request.json').read_text(encoding='utf-8'))
    identity = request['identity']
    root = pathlib.Path(identity['remote_root']).resolve(strict=True)
    pid = os.getpid()
    birth = pathlib.Path('/proc/%s/stat' % pid).read_text().rsplit(')', 1)[1].split()[19]
    atomic(directory / 'process.json', dict(pid=pid, birth=birth))
    stdout = directory / 'stdout.txt'
    stderr = directory / 'stderr.txt'
    artifacts = []
    succeeded = False
    with stdout.open('wb') as out, stderr.open('wb') as err:
        try:
            completed = subprocess.run(request['argv'], cwd=root, stdin=subprocess.DEVNULL,
                                       stdout=out, stderr=err, check=False)
            succeeded = completed.returncode == 0
            if succeeded:
                for relative in request['captures']:
                    path = (root / relative).resolve(strict=True)
                    scoped = path.relative_to(root)
                    if '.omicsops' in scoped.parts or not path.is_file():
                        raise ValueError('invalid artifact')
                    digest = hashlib.sha256()
                    with path.open('rb') as stream:
                        for block in iter(lambda: stream.read(1024 * 1024), b''):
                            digest.update(block)
                    artifacts.append(dict(relative_path=scoped.as_posix(), size_bytes=path.stat().st_size, sha256=digest.hexdigest()))
        except Exception:
            succeeded = False
            err.write(b'\nRemote job failed to execute or validate artifacts.\n')
    out_capture, err_capture = capture(stdout), capture(stderr)
    job_id = identity['job_id']
    result = dict(request_id=job_id, session_id=job_id, process_identity='ssh-detached:' + job_id,
                  stdout=out_capture['excerpt'], stderr=err_capture['excerpt'],
                  stdout_capture=out_capture, stderr_capture=err_capture, succeeded=succeeded,
                  artifacts=artifacts if succeeded else [], software_versions={})
    atomic(directory / 'result.json', dict(identity=identity, status='completed', result=result))


if __name__ == '__main__':
    worker(sys.argv[1])
"""


def job_directory(identity, create=False):
    import uuid
    job_id = str(uuid.UUID(identity['job_id']))
    root = pathlib.Path(identity['remote_root']).resolve(strict=True)
    if str(root) != identity['remote_root']:
        raise ValueError('root changed')
    base = root / '.omicsops' / 'remote-jobs'
    resolved = base.resolve()
    resolved.relative_to(root)
    if resolved != base:
        raise ValueError('symlink job directory')
    if create:
        base.mkdir(parents=True, exist_ok=True, mode=0o700)
    directory = base / job_id
    if directory.is_symlink():
        raise ValueError('symlink job identity')
    return directory


def status(identity):
    directory = job_directory(identity)
    manifest = directory / 'request.json'
    if not manifest.is_file():
        return dict(identity=identity, status='unknown')
    request = json.loads(manifest.read_text(encoding='utf-8'))
    if request['identity'] != identity:
        raise ValueError('job identity mismatch')
    result = directory / 'result.json'
    if result.is_file():
        if result.stat().st_size > 512 * 1024:
            raise ValueError('oversized receipt')
        value = json.loads(result.read_text(encoding='utf-8'))
        if value['identity'] != identity:
            raise ValueError('receipt identity mismatch')
        return value
    process = directory / 'process.json'
    if process.is_file():
        recorded = json.loads(process.read_text(encoding='utf-8'))
        try:
            stat = pathlib.Path('/proc/%s/stat' % int(recorded['pid'])).read_text().rsplit(')', 1)[1].split()
            if stat[19] == recorded['birth'] and stat[0] != 'Z':
                return dict(identity=identity, status='running')
        except (OSError, ValueError, IndexError):
            pass
    return dict(identity=identity, status='unknown')


def submit(payload):
    identity = payload['identity']
    directory = job_directory(identity, create=True)
    try:
        directory.mkdir(mode=0o700)
    except FileExistsError:
        return status(identity)  # Never relaunch an existing identity, even if incomplete.
    extension, executable = ('py', ['python', '-u']) if identity['context']['language'] == 'python' else ('R', ['Rscript', '--vanilla'])
    code_path = directory / ('code.' + extension)
    code_path.write_text(payload['code'], encoding='utf-8')
    argv = executable + [str(code_path)]
    environment = identity['context']['environment']
    if environment == 'system' and extension == 'py':
        argv[0] = sys.executable
    if environment != 'system':
        prefix = pathlib.Path(identity['remote_root']) / '.omicsops' / 'environments' / environment
        if not (prefix / 'conda-meta').is_dir():
            raise ValueError('environment unavailable')
        argv = ['micromamba', 'run', '--prefix', str(prefix)] + argv
    request = dict(identity=identity, argv=argv, captures=payload['captures'])
    (directory / 'request.json').write_text(json.dumps(request), encoding='utf-8')
    driver = directory / 'worker.py'
    driver.write_text(WORKER, encoding='utf-8')
    subprocess.Popen([sys.executable, str(driver), str(directory)], stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     close_fds=True, start_new_session=True)
    return dict(identity=identity, status='submitted')


def main(payload):
    if os.name != 'posix' or not pathlib.Path('/proc/self/stat').is_file():
        raise ValueError('detached jobs require a Linux SSH host with Python 3')
    if payload['action'] == 'submit':
        return submit(payload)
    if payload['action'] == 'status':
        return status(payload['identity'])
    raise ValueError('unknown operation')


if __name__ == '__main__':
    try:
        payload = json.loads(base64.b64decode(sys.argv[1], validate=True))
        print(json.dumps(main(payload), ensure_ascii=False))
    except Exception:
        print('remote job identity or transport operation could not be verified', file=sys.stderr)
        sys.exit(1)
