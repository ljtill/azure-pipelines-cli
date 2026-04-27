# Known limitations

## Azure DevOps pagination and caps

Azure DevOps list APIs can return partial pages and continue the result set with
an `x-ms-continuationtoken` response header. The client follows that token until
the header is absent. Empty, blank, or malformed continuation tokens are treated
as absent so a bad header does not create an endless loop.

The CLI requests bounded page sizes from ADO:

- Build definitions: `$top=1000`.
- Recent builds: `$top=1000`.
- Build history for one definition: `$top=20` per page.
- Project pull requests: `$top=100`.
- Work item comments: `$top=200`.

ADO may still return fewer items than requested or impose its own service-side
limits. Generic pagination preserves the service order and does not de-duplicate
items; endpoint-specific collectors only de-duplicate when the model requires it
(for example, retention leases are keyed by lease ID).

## Pagination safety cap

The Azure DevOps REST client refuses to follow more than 1000 continuation-token
pages for a single list endpoint, as a defense against server-side loops. This
limit is well above any realistic production workload.

Override with the `DEVOPS_MAX_PAGES` environment variable:

```sh
DEVOPS_MAX_PAGES=5000 devops
```

Values below `100` are clamped to `100`. If the cap is reached, the client
returns a typed partial-data error that includes the endpoint name, completed
page count, and item count. The refresh is not treated as a silently complete
result; raise the cap only after confirming the endpoint legitimately has that
many pages.

## Throttling and rate limits

Azure DevOps may throttle with `429 Too Many Requests` or ask clients to slow
down with `503 Service Unavailable`. Requests that are safe to replay are retried
up to three times. The client honors `Retry-After` seconds and HTTP-date values
when present, capped at 30 seconds per retry; otherwise it uses jittered
exponential backoff starting around 500 ms. Requests that may duplicate side
effects are not replayed automatically.

After retry exhaustion, `429` responses surface as rate-limit errors. When ADO
provides `Retry-After`, `X-RateLimit-Limit`, `X-RateLimit-Remaining`, or
`X-RateLimit-Reset`, those parsed values are included in diagnostics and tracing
metadata. Malformed throttling headers are ignored rather than failing the
request with a secondary parsing error.

## Degraded and partial data semantics

Current client-level partial-data behavior is explicit:

- Pagination cap failures return an error instead of silently using incomplete
  results.
- Retention lease fan-out keeps successful per-definition results and records
  failures for definitions that could not be fetched.
- Background refresh failures are surfaced through notifications and end any
  in-flight pagination progress indicator.

The UI availability model owns the final fresh/partial/stale/unavailable wording
for each view. That model is still being completed, so this document only
describes the current client-level cap and rate-limit behavior.

## Response and log caps

JSON API responses are capped at 32 MiB in memory. Plain-text build log downloads
are capped at 128 MiB; when a log exceeds that cap, the retained prefix is
returned with a truncation marker so users can open the full log in the browser.

## Log buffer cap

The log viewer keeps at most `max_log_lines` lines in memory (default `100000`, see [configuration.md](configuration.md)). For builds that produce more output than this, the oldest lines are truncated so the tail of the log is always preserved. A visible banner at the top of the log pane surfaces how many lines were dropped.
