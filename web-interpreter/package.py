"""Package the browser site with content-versioned editor assets."""
import argparse
import hashlib
from pathlib import Path
import re
import shutil

HERE = Path(__file__).resolve().parent
FILES = ('index.html', 'style.css', 'app.js', 'indent.js', 'driver.mjs',
         'worker.mjs', 'runtime-adapter.mjs', 'THIRD-PARTY-NOTICES.txt')


def package(destination, source=HERE):
    destination, source = Path(destination), Path(source)
    if destination.resolve() == source.resolve():
        raise ValueError('Package into a separate output directory.')
    if not (source / 'runtime/rfort-browser.wasm').is_file():
        raise FileNotFoundError('Build the interpreter with python web-interpreter/build.py first.')
    destination.mkdir(parents=True, exist_ok=True)
    for name in FILES:
        shutil.copyfile(source / name, destination / name)
    for name in ('vendor', 'runtime'):
        shutil.copytree(source / name, destination / name, dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('.gitignore'))
    html = (destination / 'index.html').read_text(encoding='utf-8')

    def version(match):
        attribute, path = match.groups()
        asset = destination / path
        if not asset.is_file():
            return match.group(0)
        digest = hashlib.sha256(asset.read_bytes()).hexdigest()[:16]
        return f'{attribute}="{path}?v={digest}"'

    html = re.sub(r'(src|href)="([^"?#]+\.(?:js|css))"', version, html)
    (destination / 'index.html').write_text(html, encoding='utf-8')
    (destination / '.nojekyll').touch()
    return destination


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=HERE.parent / 'target/browser-site')
    args = parser.parse_args()
    print(f'Packaged {package(args.output)}')
