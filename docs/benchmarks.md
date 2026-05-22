# Benchmarks

Lantor has focused benchmarks for composer render cost and browser input
latency. These are development tools, not product setup steps.

## Composer

Layer 1 is the fast SSR mechanism guard:

```bash
npm run bench:composer
```

Layer 2 is the browser input-latency benchmark:

```bash
npm run build:bench
npx playwright install chromium
npm run bench:composer:e2e
```

Layer 2 runs a production preview in headed Chromium, injects synthetic stress
data, and reports Chrome Event Timing duration as the primary INP-aligned
metric, with double-requestAnimationFrame input-to-frame latency as an
auxiliary fallback.

It also records long tasks, long animation frames when Chromium exposes them,
React Profiler commits in the `build:bench` profiling bundle, DOM mutations as
a fallback diagnostic, and a Playwright trace under `artifacts/`.

Use `--profile <name>` to run one profile, `--streaming-interval <ms>` to tune
synthetic streaming cadence, `--headless` only for smoke checks, and
`--no-trace` when trace output is not needed. The IME profile dispatches
simulated composition events; it does not reproduce real macOS input-method
pressure.

The Layer 2 numbers are for user-perceived typing latency investigation. Keep
them separate from the Layer 1 SSR render-cost numbers, and do not publish
before/after claims from Layer 2 until the relevant baseline is stable.
