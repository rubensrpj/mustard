# Mustard Dashboard

A React + TypeScript front end served over HTTP by `server/` — a `tiny_http`
backend in Rust. It used to be wrapped in a desktop shell; on Linux that shell
drew through WebKitGTK and was the source of the sluggishness that motivated
the move to the browser.

## Running it

```bash
pnpm --filter mustard-dashboard build       # build the React bundle into dist/
cargo run -p mustard-dashboard              # serve it at http://127.0.0.1:7777/
```

For frontend work, run the two halves side by side: `cargo run -p
mustard-dashboard` for the data and `pnpm --filter mustard-dashboard dev` for
hot reload. The dev server proxies `/api` to the backend, so the frontend's
URLs stay relative and identical in both modes. Point the proxy elsewhere with
`MUSTARD_DASHBOARD_PORT` — the same variable the server reads.

The server scans for projects from the directory it was started in (`--root`
overrides). `--port`, `--host` and `--no-open` are the rest of the surface;
`--help` prints them. Exposing the dashboard to the network requires naming
`--host` explicitly — it reads the `.claude/` of every project on the machine.

## Recommended IDE setup

- [VS Code](https://code.visualstudio.com/) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
