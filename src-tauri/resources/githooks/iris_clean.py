#!/usr/bin/env python3
"""Git clean filter for config/iris.properties: drops Iris's save timestamp and commits shaders disabled."""
import re
import sys

TIMESTAMP = re.compile(r"^#\w{3} \w{3} \d{1,2} \d{2}:\d{2}:\d{2} \w+ \d{4}\s*$")

for line in sys.stdin.read().splitlines():
    if TIMESTAMP.match(line):
        continue
    if line.startswith("enableShaders="):
        line = "enableShaders=false"
    sys.stdout.write(line + "\n")
