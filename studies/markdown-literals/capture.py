"""Run actual CLI reports through GitHub's cmark-gfm reference parser.

All inputs are synthetic. The parser and CLI are local binaries; no network use.
Usage: python3 capture.py --binary PATH --cmark PATH --out DIRECTORY [--check]
"""
import argparse
import hashlib
import json
import string
import subprocess
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path


class Rendered(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.tags = Counter()
        self.code_texts = []
        self.code = None
        self.code_children = 0
        self.tables = []
        self.row = None

    def handle_starttag(self, tag, attrs):
        self.tags[tag] += 1
        if self.code is not None:
            self.code_children += 1
        if tag == 'code':
            self.code = ''
        if tag == 'table':
            self.tables.append([])
        if tag == 'tr':
            self.row = 0
        if tag in ('th', 'td'):
            self.row += 1

    def handle_endtag(self, tag):
        if tag == 'code' and self.code is not None:
            self.code_texts.append(self.code)
            self.code = None
        if tag == 'tr':
            self.tables[-1].append(self.row)
            self.row = None

    def handle_data(self, data):
        if self.code is not None:
            self.code += data


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')) + '\n'


def cases():
    image = '![PASS](https://synthetic.invalid/pass.svg)'
    raw = f'ERROR result `{image}` denied'
    yield dict(name='flat-image', baseline='boot ready\n', target=raw + '\n', args=['--flat'])
    yield dict(name='gone-image', baseline=raw + '\n', target='boot ready\n', args=['--flat'])
    yield dict(name='new-value-backticks', baseline='job `PASSED` done\n' * 10,
               target='job `FAILED` done\n' * 10, args=['--flat'], require_value=True)
    values = {
        'image': image,
        'links-and-email': '[audit](https://synthetic.invalid/audit) https://synthetic.invalid test@example.com',
        'formatting': '**failed** _error_ ~~removed~~',
        'backtick-runs': '`start ``middle``` end`',
        'pipes-and-backslashes': r'a|b\|c\\|d\\\|e',
        'html-and-entities': '<img src="https://synthetic.invalid/x"> &amp; &#96; <script>bad</script>',
        'unicode-and-spaces': '  café 雪 😀\tquoted\nline  ',
        'all-punctuation': string.punctuation,
        'clipped-backticks': '`' + 'x' * 398 + '`![tail](https://synthetic.invalid/tail)',
        'empty': '',
    }
    for name, value in values.items():
        yield dict(name='field-' + name, baseline=encoded({'v': 'baseline'}), target=encoded({'v': value}), args=['--watch-field', '/v'])
    key = 'g|`![HEADER](header.svg)`'
    yield dict(name='group-header', baseline=encoded({key: 'old', 'v': 200}),
               target=encoded({key: image, 'v': 503}), args=['--watch-field', '/v', '--watch-by', '/' + key])
    yield dict(name='incomplete-evidence', baseline=encoded({'v': 200}), target=f'not-json `{image}`\n',
               args=['--watch-field', '/v'], incomplete=True)
    yield dict(name='known-value-rates', baseline=encoded({'g': image, 'v': 'ok'}) * 198 + encoded({'g': image, 'v': image}) * 2,
               target=encoded({'g': image, 'v': 'ok'}) * 160 + encoded({'g': image, 'v': image}) * 40,
               args=['--watch-field', '/v', '--watch-by', '/g', '--watch-rate-change', '5'], require_rate=True)
    filename = 'run`![PATH](path.svg)`|capture.log'
    yield dict(name='filename-flat', baseline='boot ready\n', target=raw + '\n', args=['--flat'], filename=filename)
    # Real repository fixture structure, with a synthetic filename. Self-contained copies
    # are already part of the crate's documented examples.
    root = Path(__file__).resolve().parents[2]
    yield dict(name='filename-block', baseline=(root / 'examples/pytest-pass.log').read_text(),
               target=(root / 'examples/pytest-fail.log').read_text(), args=['--block-lines', '0'], filename=filename, require_block=True)
    yield dict(name='filename-newline', baseline='boot ready\n', target=raw + '\n', args=['--flat'], filename='run`\n**FORGED**\n`|capture.log')
    yield dict(name='ordinary-control', baseline='boot ready\n', target='boot ready\n', args=[])


def shown(text):
    # Fixtures have no ANSI controls. The report flattens filename newlines.
    text = text.replace('\n', ' ').replace('\r', ' ')
    return text if len(text) <= 400 else text[:400] + f'… (+{len(text) - 400} more characters)'


def required_literals(report, filename):
    wanted = ['baseline.log', filename]
    for finding in report['findings']:
        if 'block' not in finding:
            wanted += [finding['template']]
            wanted += [finding[key] for key in ('first_target_raw', 'first_baseline_raw') if isinstance(finding.get(key), str)]
    for value in report['value_findings']:
        if 'block' not in value:
            wanted += [value['template'], value['new_value'], ', '.join(value['baseline_values']), value['first_target_raw']]
    for field in report.get('watched_fields', []):
        wanted += [field['pointer'], *field.get('group_by', [])]
        for value in field['values']:
            wanted += [value['value_json'], *value.get('group_values_json', [])]
        for coverage in [*field['baselines'], field['target']]:
            if 'first_problem' in coverage:
                wanted.append(coverage['first_problem']['raw'])
    return sorted(set(map(shown, wanted)))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--cmark', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    binary, cmark, out = args.binary.resolve(), args.cmark.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    records = []
    for case in cases():
        work = out / case['name']
        work.mkdir(exist_ok=True)
        filename = case.get('filename', 'target.log')
        (work / 'baseline.log').write_text(case['baseline'], encoding='utf-8')
        (work / filename).write_text(case['target'], encoding='utf-8')
        command = [str(binary), 'diff', 'baseline.log', '--target', filename, *case['args']]
        runs = {}
        for format, flags in [('markdown', ['--markdown']), ('json', ['--json']), ('terminal', ['--color', 'never'])]:
            run = subprocess.run(command + flags, cwd=work, capture_output=True, timeout=20)
            assert run.returncode in (0, 1, 2), (case['name'], run.stderr)
            assert not run.stderr, (case['name'], run.stderr)
            runs[format] = run
            (work / {'markdown': 'report.md', 'json': 'report.json', 'terminal': 'report.txt'}[format]).write_bytes(run.stdout)
        report = json.loads(runs['json'].stdout)
        rendered = subprocess.run([str(cmark), '--unsafe', '-e', 'table', '-e', 'autolink', '-e', 'strikethrough', '-e', 'tagfilter'],
                                  input=runs['markdown'].stdout, capture_output=True, timeout=10, check=True).stdout
        (work / 'report.html').write_bytes(rendered)
        dom = Rendered()
        dom.feed(rendered.decode())
        missing = [text for text in required_literals(report, filename) if text not in dom.code_texts]
        expected_exit = 2 if case.get('incomplete') else 0 if case['name'] == 'ordinary-control' else 1
        checks = dict(expected_exit=all(run.returncode == expected_exit for run in runs.values()),
                      literal_evidence_preserved=not missing,
                      no_links_or_images=dom.tags['a'] == dom.tags['img'] == 0,
                      code_has_no_elements=dom.code_children == 0,
                      no_source_formatting=all(dom.tags[tag] == 0 for tag in ['em', 'del', 'script', 'iframe']),
                      table_columns_preserved=all(len(set(rows)) == 1 for rows in dom.tables),
                      expected_value_path=not case.get('require_value') or bool(report['value_findings']),
                      expected_block_path=not case.get('require_block') or bool(report['blocks']),
                      expected_rate_path=not case.get('require_rate') or any(group['changes'] for field in report.get('watched_fields', []) for group in field.get('rate_comparison', {}).get('groups', [])))
        records.append(dict(name=case['name'], checks=checks, agrees=all(checks.values()), missing_code_text=missing,
                            image_elements=dom.tags['img'], link_elements=dom.tags['a'], code_children=dom.code_children,
                            code_text_sha256=hashlib.sha256(json.dumps(dom.code_texts, ensure_ascii=False, separators=(',', ':')).encode()).hexdigest(),
                            outputs={format: dict(exit=run.returncode, bytes=len(run.stdout), sha256=hashlib.sha256(run.stdout).hexdigest()) for format, run in runs.items()},
                            rendered_html=dict(bytes=len(rendered), sha256=hashlib.sha256(rendered).hexdigest())))
    receipt = dict(scope='Synthetic actual CLI captures rendered locally by cmark-gfm; no hosted posting or network requests.',
                   cli_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                   parser_version=subprocess.check_output([str(cmark), '--version'], text=True).strip(),
                   parser_sha256=hashlib.sha256(cmark.read_bytes()).hexdigest(),
                   cases=len(records), disagreements=sum(not r['agrees'] for r in records), results=records)
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({key: receipt[key] for key in ['cases', 'disagreements', 'cli_sha256']}))
    for record in records:
        if not record['agrees']:
            print(record['name'], [name for name, ok in record['checks'].items() if not ok])
    if args.check and receipt['disagreements']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
