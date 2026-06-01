#!/usr/bin/env python
"""Start the complete demo environment."""

import runpy
import sys
from pathlib import Path


if __name__ == "__main__":
    target = Path(__file__).with_name("demo-start.py")
    sys.argv[0] = str(target)
    runpy.run_path(str(target), run_name="__main__")
