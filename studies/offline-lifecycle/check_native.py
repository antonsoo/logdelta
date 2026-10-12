"""Compare retained synthetic browser results with an installed native executable."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--browser', type=Path, required=True)
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
binary = args.binary.resolve()
data = json.loads(args.browser.read_text())
cases = []
with tempfile.TemporaryDirectory(prefix='logdelta-offline-cli-') as directory:
    root = Path(directory)
    (root / 'good.log').write_text(data['inputs']['good'])
    for repetitions in (1, 2):
        (root / 'bad.log').write_text(data['inputs']['bad'] * repetitions)
        command = [str(binary), 'diff', 'good.log', '--target', 'bad.log',
                   '--watch-field', '/status', '--watch-by', '/route', '--context', '2', '--json']
        completed = subprocess.run(command, cwd=root, capture_output=True, check=False, timeout=30)
        assert completed.returncode == 1, completed.stderr
        result = json.loads(completed.stdout)
        reports = [report for case in data['results'] for report in case['reports']
                   if report['result']['target_total'] == repetitions]
        assert reports, repetitions
        assert all(report['result'] == result for report in reports), repetitions
        cases.append({'target_repetitions': repetitions, 'browser_reports': len(reports),
                      'native_exit': completed.returncode, 'result': result})
args.out.parent.mkdir(parents=True, exist_ok=True)
args.out.write_text(json.dumps({'synthetic': True, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                                'cases': cases}, indent=2) + '\n')
print('All', sum(case['browser_reports'] for case in cases), 'browser reports match the native executable')
