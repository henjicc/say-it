# Privileged and reverse-engineered providers

Use this path for providers that require a login WebView, first-party cookies, captured device parameters, or unofficial browser endpoints.

## Required separation

- Keep the browser and reverse-engineered protocol inside the plugin package. Do not inject scripts into SayIt's main or indicator WebView.
- Request only `browserSession`, `cookies`, and `network` permissions actually needed by the plugin.
- Use only a session created by an explicit user login. Provide logout and session-expired behavior inside the plugin.
- Store session material in an OS-protected credential store owned by the plugin. Do not put cookies or tokens in `manifest.json`, logs, command lines, or stdout.
- Treat captured device IDs and request parameters as versioned upstream data. Validate them before recognition and fail with an actionable error when stale.

## API v1 limitation

SayIt v1 declares privileged permissions but does not lend its own WebView or cookie jar to plugins. A privileged plugin must bundle its own login/setup executable or defer integration until the host adds a privileged action API. Never pretend that declaring `browserSession` automatically grants cookies.

## Adaptation checklist

- Identify the exact user-visible login step.
- Record which domains are accessed and why.
- Distinguish durable credentials from short-lived tokens.
- Implement expiry detection and a re-login prompt.
- Confirm the ASR stream still maps cleanly to `ready`, `partial`, `final`, `finished`, and `error`.
- Add compatibility diagnostics for upstream schema or endpoint changes.
- Avoid CAPTCHA automation, fingerprint spoofing, risk-control bypass, or harvesting unrelated browser data.
