#!/bin/bash
source $(dirname $0)/executor.sh

APP=1rpc execute $@
