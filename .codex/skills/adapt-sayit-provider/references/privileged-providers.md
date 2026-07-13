# Privileged and reverse-engineered providers

Use this host path when a provider requires first-party网页登录、Cookie、页面运行时生成的设备参数，或非官方浏览器接口。Do not add provider-specific login code to SayIt.

## Generic host contract

Declare `browserSession` and both `browserSession`/`cookies` permissions:

```json
{
  "provider": {
    "actions": ["openLogin", "syncSession", "clearSession", "diagnose"]
  },
  "runtime": {
    "permissions": ["network", "browserSession", "cookies"]
  },
  "browserSession": {
    "loginUrl": "https://vendor.example/login",
    "allowedUrls": ["https://vendor.example/", "https://ws.vendor.example/"],
    "windowTitle": "Vendor 登录",
    "userAgent": "optional exact browser UA",
    "initializationScript": "optional capture hook, maximum 64 KiB"
  }
}
```

- `openLogin` creates a dedicated persistent WebView, never SayIt's main/indicator WebView.
- `syncSession` reads cookies only for declared HTTPS URLs, serializes them without exposing values to React, and encrypts them with Windows DPAPI for the current OS user.
- The decrypted session is passed to the connector only through JSONL stdin as `session`; it is never placed in config, command-line arguments or environment variables.
- `clearSession` removes the encrypted snapshot and clears the dedicated WebView data.
- Other declared actions are sent to the connector as the `action` operation.

An initialization script may observe the page's own fetch/XHR query parameters and write a short-lived first-party marker Cookie. The connector, not the host, interprets that marker. Limit the hook to declared vendor hosts and never collect unrelated browsing data.

## Reverse-engineered provider rules

- Use only sessions created by explicit user login; never import another browser profile silently.
- Separate durable cookies, short-lived tokens and captured device/request parameters.
- Detect expiry and return a clear re-login error.
- Pin endpoint/schema assumptions and provide a `diagnose` action that reports compatibility without secrets.
- Do not automate CAPTCHA solving, fingerprint spoofing, risk-control bypass, account creation or credential harvesting.
- Keep all unofficial URL construction, signing, WebSocket parsing and upstream fallbacks in the connector executable.

## Acceptance test

Confirm login-window isolation, allowed-domain cookie collection, DPAPI persistence across SayIt restart, session clearing, main-window-independent ASR, upstream-schema failure diagnostics and zero secrets in stdout/stderr/domain events.
