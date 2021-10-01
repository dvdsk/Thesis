#!/usr/bin/env bash

url="https://github.com/jaegertracing/jaeger/releases/download/v1.25.0/jaeger-1.25.0-linux-amd64.tar.gz"
archive_path="jaeger-1.25.0-linux-amd64/jaeger-all-in-one"

curl -L $url | tar -zxvf - $archive_path
mv $archive_path "jeager-all-in-one"
rm -r "$(dirname $archive_path)"
