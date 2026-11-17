#!/usr/bin/env bash

node -v | grep -q '^v24' || (echo "Node version must be ^24 to lint this package. Run \`nvm use 24\` to switch to the correct version." && exit 1)
