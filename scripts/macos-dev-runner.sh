#!/bin/sh
# tauri:dev 在 macOS 上的 cargo runner：启动开发版前用固定的开发证书签名，再 exec 成开发版本身。
# 链接器给的 ad-hoc 签名按代码哈希认身份，每次重编都变，麦克风、辅助功能、输入监控就得重新授权；
# 用证书签名后身份是「标识 + 证书指纹」，授权一次后跨编译保留。
# 证书是自签名的，放在仓库内已忽略的独立钥匙串里，不加入钥匙串搜索列表，也不需要系统信任。
set -e

identifier="com.henjicc.sayit.dev"
identity="SayIt Development"
dir="$(cd "$(dirname "$0")/.." && pwd)/工作区/dev-signing"
keychain="$dir/dev-signing.keychain-db"
# 只保护这份本地开发钥匙串，里面只有上面这张开发证书。
password="sayit-dev"

if [ ! -f "$keychain" ]; then
  mkdir -p "$dir"
  work="$(mktemp -d)"
  cat > "$work/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $identity
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.cnf" \
    -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
  openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
    -out "$work/identity.p12" -passout "pass:$password"
  security create-keychain -p "$password" "$keychain"
  security import "$work/identity.p12" -k "$keychain" -P "$password" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
  rm -rf "$work"
  echo "dev-signing: 已生成开发签名证书，首次运行需要重新授予一次麦克风等权限。"
fi

security unlock-keychain -p "$password" "$keychain"
# 内容没变时 cargo 不会重写二进制，签名仍有效，不必重签。
if ! codesign -d -r- "$1" 2>&1 | grep -q "identifier \"$identifier\" and certificate leaf"; then
  codesign -f -s "$identity" --keychain "$keychain" -i "$identifier" "$1" 2>/dev/null
fi
exec "$@"
