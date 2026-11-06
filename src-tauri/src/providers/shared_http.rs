use std::sync::Mutex;

use hyper_util::client::proxy::matcher::Matcher;

/// 进程内共用的 HTTP 客户端：同一域名的后续请求复用连接池，不必重做 TLS 握手（走代理时约 1.5 秒）。
///
/// reqwest 只在建客户端时读一次系统代理，所以每次取用都按当前系统代理比对，变了就换新客户端，
/// 用户运行中开关或切换代理后下一个请求即生效。比对用的是 reqwest 内部同一套读取逻辑；
/// 它不含代理账号密码，只改代理凭据时要重启应用才生效。
pub(crate) struct SharedHttpClient<C> {
    build: fn() -> C,
    current: Mutex<Option<(String, C)>>,
}

impl<C: Clone> SharedHttpClient<C> {
    pub(crate) const fn new(build: fn() -> C) -> Self {
        Self {
            build,
            current: Mutex::new(None),
        }
    }

    pub(crate) fn get(&self) -> C {
        let proxy = format!("{:?}", Matcher::from_system());
        let mut current = self.current.lock().unwrap();
        if let Some((key, client)) = current.as_ref() {
            if *key == proxy {
                return client.clone();
            }
        }
        let client = (self.build)();
        *current = Some((proxy, client.clone()));
        client
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static BUILDS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn reuses_client_until_system_proxy_changes() {
        let shared = SharedHttpClient::new(|| BUILDS.fetch_add(1, Ordering::SeqCst));
        assert_eq!(shared.get(), shared.get());
        // 只改绕过列表，不影响其他用例实际走的代理。
        let previous = std::env::var_os("NO_PROXY");
        std::env::set_var("NO_PROXY", "sayit-shared-http-test.invalid");
        let changed = shared.get();
        match previous {
            Some(value) => std::env::set_var("NO_PROXY", value),
            None => std::env::remove_var("NO_PROXY"),
        }
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
        assert_eq!(changed, 1);
    }
}
