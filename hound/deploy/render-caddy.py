#!/usr/bin/env python3
"""Replace only the old Datadog site block in the host Caddyfile."""

from pathlib import Path

config = Path('/etc/caddy/Caddyfile')
template = Path('/root/hound-caddy-site.template').read_text()
password_hash = Path('/root/hound-password.hash').read_text().strip()
replacement = template.replace('__HOUND_PASSWORD_HASH__', password_hash)
lines = config.read_text().splitlines(keepends=True)
start = next(i for i, line in enumerate(lines) if line.strip() == 'datadog.properrobotics.org {')
end = next(i + 1 for i in range(start + 1, len(lines)) if lines[i].startswith('}'))
Path('/etc/caddy/Caddyfile.hound-next').write_text(''.join(lines[:start]) + replacement + ''.join(lines[end:]))
