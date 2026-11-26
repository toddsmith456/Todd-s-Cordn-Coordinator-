import hashlib, json, os, sys, urllib.request, urllib.error, urllib.parse
from pathlib import Path

repo = os.environ['GITHUB_REPOSITORY']
api = 'https://api.github.com/repos/' + repo
tag = os.environ['GITHUB_REF_NAME']
headers = {
    'Authorization': 'Bearer ' + os.environ['GH_TOKEN'],
    'Accept': 'application/vnd.github+json',
    'User-Agent': 'Todd-Cordn-release',
    'X-GitHub-Api-Version': '2022-11-28',
}

def request(url, data=None, content_type='application/json', method=None):
    h = dict(headers)
    h['Content-Type'] = content_type
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=120) as response:
            body = response.read()
            if not body:
                return {}
            return json.loads(body)
    except urllib.error.HTTPError as e:
        message = e.read().decode(errors='replace')
        print('::error::GitHub API HTTP ' + str(e.code) + ' ' + url + ': ' + message.replace('\n', ' ').replace('\r', ' '))
        raise

folder = Path('artifacts')
apks = sorted(folder.rglob('*.apk'))
print('Found APKs:')
for apk in apks:
    print(f'  {apk} ({apk.stat().st_size} bytes)')
if not apks:
    print('Contents of artifacts directory:')
    for p in sorted(folder.rglob('*')):
        print(f'  {p}')
    raise SystemExit('No verified APKs found')

# find or create release
release = None
try:
    release = request(api + '/releases/tags/' + urllib.parse.quote(tag, safe=''))
    print('Found existing release for tag', tag)
except urllib.error.HTTPError as e:
    if e.code != 404:
        raise
    print('No existing release for tag, creating...')

if release is None:
    notes_path = Path('docs/android-release-notes.md')
    body = notes_path.read_text() if notes_path.exists() else f'Android release {tag}'
    release = request(api + '/releases', json.dumps({
        'tag_name': tag,
        'name': "Todd's Coordinator " + tag.removeprefix('android-v'),
        'body': body,
        'draft': False,
        'prerelease': '-' in tag.removeprefix('android-v'),
    }).encode())
    print('Created release', release.get('html_url'))

# refresh assets list
release = request(api + '/releases/tags/' + urllib.parse.quote(tag, safe=''))
uploaded = {a['name']: a for a in release.get('assets', [])}
upload_base = release['upload_url'].split('{')[0]

for path in apks:
    if path.name in uploaded:
        print('Already uploaded:', path.name)
        continue
    upload = upload_base + '?name=' + urllib.parse.quote(path.name)
    print('Uploading', path.name)
    request(upload, path.read_bytes(), 'application/vnd.android.package-archive')
    print('Uploaded', path.name)

release = request(api + '/releases/tags/' + urllib.parse.quote(tag, safe=''))
print('Published:', release.get('html_url'))
