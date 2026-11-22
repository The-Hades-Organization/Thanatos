#!/usr/bin/env bash
cat <<'JSON'
{"v":1,"events":[{"ts":"2026-03-01T11:30:00-03:00","text":"erro 502 no gateway token=abcdef123456","level":"error"},{"ts":"2026-03-01T14:10:00Z","text":"retry ativo"}],"metrics":[{"name":"p99","value":3.2,"unit":"s","ts":"2026-03-01T14:00:00Z"}]}
JSON
