import hashlib, json, os, sys, urllib.request, urllib.error, urllib.parse
from pathlib import Path
repo = os.environ['GITHUB_REPOSITORY']
api = 'https://api.github.com/repos/' + repo
tag = os.environ['GITHUB_REF_NAME']
headers = {'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json', 'User-Agent': 'Todd-Cordn-release', 'X-GitHub-Api-Version': '2022-11-28'}
def request(url, data=None, content_type='application/json'):
    h = dict(headers)
    h['Content-Type'] = content_type
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=h), timeout=120) as response:
            return json.load(response)
    except urllib.error.HTTPError as e:
        message = e.read().decode(errors='replace')
        print('::error::GitHub release API HTTP ' + str(e.code) + ': ' + message.replace('\n', ' ').replace('\r', ' '))
        raise SystemExit(1)
folder = Path('artifacts')
apks = sorted(folder.glob('*.apk'))
if not apks:
    raise SystemExit('No verified APKs found')
checksums = ''.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name + '\n' for p in apks)
(folder / 'SHA256SUMS').write_text(checksums)
releases = request(api + '/releases')
release = next((r for r in releases if r['tag_name'] == tag), None)
if release is None:
    release = request(api + '/releases', json.dumps({
        'tag_name': tag,
        'name': 'Todd’s Cordn Coordinator ' + tag.removeprefix('android-v'),
        'body': Path('docs/android-release-notes.md').read_text(),
        'draft': False,
        'prerelease': '-' in tag.removeprefix('android-v'),
    }).encode())
uploaded = {a['name'] for a in release.get('assets', [])}
for path in apks + [folder / 'SHA256SUMS']:
    if path.name in uploaded:
        continue
    upload = release['upload_url'].split('{')[0] + '?name=' + urllib.parse.quote(path.name)
    request(upload, path.read_bytes(), 'application/vnd.android.package-archive' if path.suffix == '.apk' else 'text/plain')
print('Published:', release['html_url'])
