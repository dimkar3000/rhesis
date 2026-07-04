#!/bin/bash
set -euo pipefail

JDK_DIR="/app/jdk"
JRE_DIR="/app/jre"

/app/jdk/bin/jlink --module-path /app/jdk/jmods \
    --add-modules java.base,java.logging,java.xml,java.naming,java.management,java.sql,jdk.httpserver,jdk.unsupported,java.desktop,java.net.http,java.scripting,java.compiler,java.prefs,java.rmi,java.security.jgss,java.security.sasl,java.instrument \
    --output /app/jre \
    --strip-debug \
    --compress=2

rm -rf /app/jdk
