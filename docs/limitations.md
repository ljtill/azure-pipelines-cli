# Known limitations

## Log buffer cap

The log viewer keeps at most `max_log_lines` lines in memory (default `100000`, see [configuration.md](configuration.md)). For builds that produce more output than this, the oldest lines are truncated so the tail of the log is always preserved. A visible banner at the top of the log pane surfaces how many lines were dropped.

## Pagination safety cap

The Azure DevOps REST client refuses to follow more than 1000 continuation-token pages for a single list endpoint, as a defense against server-side loops. This limit is well above any realistic production workload.

Override with the `DEVOPS_MAX_PAGES` environment variable:

```sh
DEVOPS_MAX_PAGES=5000 devops
```

Values below `100` are clamped to `100`.
