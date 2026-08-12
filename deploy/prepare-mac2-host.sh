#!/bin/sh
set -eu

/usr/bin/sudo -n /usr/bin/automationmodetool enable-automationmode-without-authentication
/usr/bin/automationmodetool
