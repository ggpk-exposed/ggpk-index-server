#!/bin/bash

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get -y install --no-install-recommends libssl-dev
apt-get clean
rm -rf /var/lib/apt/lists/*