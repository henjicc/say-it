use super::*;
use tokio::io::AsyncReadExt;

// SDK 校验各供应商限额；此处只限定桥接资源，不把整文件放进 QuickJS。
const MAX_UPLOAD_BODY_BYTES: u64 = 3_000_000_000;
const MAX_ACTIVE_UPLOADS: usize = 4;

pub(super) struct UploadState {
    sender: Option<mpsc::Sender<Vec<u8>>>,
    response: Option<tokio::sync::oneshot::Receiver<Result<reqwest::Response, String>>>,
    stop: tokio_util::sync::CancellationToken,
    expected: u64,
    written: u64,
}

impl Drop for UploadState {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) struct MediaInput {
    file: std::fs::File,
    metadata: std::fs::Metadata,
    mime_type: String,
    filename: String,
}

impl MediaInput {
    fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > 9_007_199_254_740_991 {
            return Err("媒体句柄必须指向大小可精确表示的普通文件".into());
        }
        Ok(Self {
            file,
            metadata,
            mime_type: media_mime_type(path).into(),
            filename: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("media")
                .into(),
        })
    }

    fn check_unchanged(&self) -> Result<(), String> {
        check_metadata(
            &self.metadata,
            &self.file.metadata().map_err(|error| error.to_string())?,
        )
    }

    pub(super) fn describe(&self) -> Result<Value, String> {
        self.check_unchanged()?;
        Ok(
            json!({"size": self.metadata.len(), "mimeType": self.mime_type, "filename": self.filename}),
        )
    }

    pub(super) fn read_chunk(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        self.check_unchanged()?;
        let size = self.metadata.len();
        if offset >= size {
            return Ok(Vec::new());
        }
        let mut bytes = vec![0; length.min((size - offset).min(usize::MAX as u64) as usize)];
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.check_unchanged()?;
        Ok(bytes)
    }
}

fn check_metadata(before: &std::fs::Metadata, after: &std::fs::Metadata) -> Result<(), String> {
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("上传期间媒体文件已发生变化，请重新选择文件".into());
    }
    Ok(())
}

impl HostState {
    pub(super) fn sdk_media_input(&mut self, reference: &str) -> Result<&mut MediaInput, String> {
        if !self.media_inputs.contains_key(reference) {
            let path = self.inputs.get(reference).ok_or("无效或过期的媒体句柄")?;
            self.media_inputs
                .insert(reference.into(), MediaInput::open(path)?);
        }
        self.media_inputs
            .get_mut(reference)
            .ok_or_else(|| "媒体句柄已关闭".into())
    }

    fn upload_request(&self, payload: &Value) -> Result<reqwest::RequestBuilder, String> {
        if self.sdk.is_none() {
            return Err("当前运行上下文未注入 SDK 传输作用域".into());
        }
        require_network_permission(&self.spec)?;
        let url = parse_allowed_url(
            &self.spec,
            payload
                .get("url")
                .and_then(Value::as_str)
                .ok_or("上传缺少 url")?,
            &["https"],
        )?;
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("POST")
            .parse::<reqwest::Method>()
            .map_err(|_| "非法 HTTP 方法")?;
        // 请求体是一次性流，禁止重定向重放或把凭据带往其他端点。
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let mut request = client.request(method, url);
        if let Some(headers) = payload.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if name.eq_ignore_ascii_case("content-length")
                    || name.eq_ignore_ascii_case("transfer-encoding")
                {
                    continue;
                }
                request = request.header(name, value.as_str().ok_or("HTTP header 必须为字符串")?);
            }
        }
        Ok(request)
    }

    pub(super) fn upload_open(&mut self, payload: Value) -> Result<Value, String> {
        if self.uploads.len() >= MAX_ACTIVE_UPLOADS {
            return Err("并行上传数量超过限制".into());
        }
        let expected = payload
            .get("contentLength")
            .and_then(Value::as_u64)
            .filter(|length| *length <= MAX_UPLOAD_BODY_BYTES)
            .ok_or("非法流式请求体长度")?;
        let request = self.upload_request(&payload)?;
        // 只容纳一个分块；生产者必须等待网络消费者，内存不随文件增长。
        let (sender, receiver) = mpsc::channel::<Vec<u8>>(1);
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver
                .recv()
                .await
                .map(|chunk| (Ok::<_, std::io::Error>(chunk), receiver))
        });
        let request = request
            .header(reqwest::header::CONTENT_LENGTH, expected)
            .body(reqwest::Body::wrap_stream(stream));
        self.start_upload(request, Some(sender), expected)
    }

    fn start_upload(
        &mut self,
        request: reqwest::RequestBuilder,
        sender: Option<mpsc::Sender<Vec<u8>>>,
        expected: u64,
    ) -> Result<Value, String> {
        if self.uploads.len() >= MAX_ACTIVE_UPLOADS {
            return Err("并行上传数量超过限制".into());
        }
        let (response_tx, response) = tokio::sync::oneshot::channel();
        let stop = tokio_util::sync::CancellationToken::new();
        let worker_stop = stop.clone();
        let cancelled = self.cancelled.clone();
        let deadline = self.deadline.clone();
        tauri::async_runtime::spawn(async move {
            let result = tokio::select! {
                result = request.send() => result.map_err(|error| error.to_string()),
                _ = worker_stop.cancelled() => Err("上传已关闭".into()),
                _ = wait_for_stop(cancelled.clone(), deadline.clone()) => Err(stop_reason(&cancelled, &deadline)),
            };
            let _ = response_tx.send(result);
        });
        let id = uuid::Uuid::new_v4().to_string();
        self.uploads.insert(
            id.clone(),
            UploadState {
                sender,
                response: Some(response),
                stop,
                expected,
                written: 0,
            },
        );
        Ok(json!({"uploadId":id}))
    }

    pub(super) fn upload_write(&mut self, payload: Value) -> Result<Value, String> {
        let id = upload_id(&payload)?;
        let body_id = payload
            .get("bodyBufferId")
            .and_then(Value::as_str)
            .ok_or("上传缺少二进制分块")?;
        let bytes = self
            .pending_request_bodies
            .remove(body_id)
            .ok_or("上传分块已过期")?;
        if bytes.is_empty() || bytes.len() > MAX_MEDIA_CHUNK_BYTES {
            return Err("上传分块必须在 1 到 64 KiB 之间".into());
        }
        let upload = self.uploads.get_mut(id).ok_or("上传不存在或已关闭")?;
        if upload.written + bytes.len() as u64 > upload.expected {
            return Err("上传超过声明的请求体长度".into());
        }
        let count = bytes.len() as u64;
        let sender = upload.sender.as_ref().ok_or("上传已结束")?;
        match sender.try_send(bytes) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Ok(json!({"ready":false})),
            // 服务端可以在请求体发送完前返回 413/401 等响应，仍应按 HTTP 响应交给 SDK。
            Err(mpsc::error::TrySendError::Closed(_)) => return self.upload_response(id),
        }
        upload.written += count;
        Ok(json!({"ready":true}))
    }

    pub(super) fn upload_finish(&mut self, payload: Value) -> Result<Value, String> {
        let id = upload_id(&payload)?;
        let upload = self.uploads.get_mut(id).ok_or("上传不存在或已关闭")?;
        if upload.written != upload.expected {
            return Err("上传未达到声明的请求体长度".into());
        }
        upload.sender.take();
        self.upload_response(id)
    }

    fn upload_response(&mut self, id: &str) -> Result<Value, String> {
        let upload = self.uploads.get_mut(id).ok_or("上传不存在或已关闭")?;
        let result = upload.response.as_mut().ok_or("上传已结束")?.try_recv();
        let response = match result {
            Ok(result) => result,
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                return Ok(json!({"ready":false}))
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                Err("上传任务意外退出".into())
            }
        };
        self.uploads.remove(id);
        Ok(json!({"ready":true,"response":self.register_http_response(response?)}))
    }

    pub(super) fn upload_close(&mut self, payload: Value) -> Result<Value, String> {
        self.uploads.remove(upload_id(&payload)?);
        Ok(Value::Null)
    }

    pub(super) fn upload_file(&mut self, payload: Value) -> Result<Value, String> {
        let request = self.upload_request(&payload)?;
        let reference = payload
            .get("ref")
            .and_then(Value::as_str)
            .ok_or("上传缺少 ref")?;
        let path = self.inputs.get(reference).ok_or("无效或过期的媒体句柄")?;
        // 在网络开始前打开受作用域约束的文件，之后始终复用同一文件句柄。
        let input = MediaInput::open(path)?;
        let size = input.metadata.len();
        let max_bytes = payload
            .get("maxBytes")
            .and_then(Value::as_u64)
            .filter(|size| *size > 0 && *size <= MAX_UPLOAD_BODY_BYTES)
            .ok_or("非法上传大小上限")?;
        if size > max_bytes {
            return Ok(
                json!({"error": {"code":"media_too_large", "message":"录音文件过大，请分段后重试",
                "details":{"actualBytes":size,"maxBytes":max_bytes}}}),
            );
        }
        let fields: Vec<(String, String)> =
            serde_json::from_value(payload.get("fields").cloned().ok_or("上传缺少 fields")?)
                .map_err(|_| "上传字段必须为字符串键值对")?;
        if fields
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > 1024 * 1024
        {
            return Err("上传字段超过 1 MiB 限制".into());
        }
        let file_field = payload
            .get("fileField")
            .and_then(Value::as_str)
            .filter(|field| !field.is_empty())
            .ok_or("上传缺少 fileField")?
            .to_string();
        // File::from_std 仅包装句柄，不执行阻塞 I/O。
        let file = tokio::fs::File::from_std(input.file);
        let metadata = input.metadata;
        let stream = futures_util::stream::try_unfold((file, 0_u64), move |(mut file, offset)| {
            let metadata = metadata.clone();
            async move {
                let current = file.metadata().await?;
                check_metadata(&metadata, &current).map_err(std::io::Error::other)?;
                if offset == size {
                    return Ok::<_, std::io::Error>(None);
                }
                let mut bytes = vec![0; ((size - offset) as usize).min(MAX_MEDIA_CHUNK_BYTES)];
                file.read_exact(&mut bytes).await?;
                check_metadata(&metadata, &file.metadata().await?)
                    .map_err(std::io::Error::other)?;
                let next = offset + bytes.len() as u64;
                Ok(Some((bytes, (file, next))))
            }
        });
        let mut form = reqwest::multipart::Form::new();
        for (key, value) in fields {
            form = form.text(key, value);
        }
        let part =
            reqwest::multipart::Part::stream_with_length(reqwest::Body::wrap_stream(stream), size)
                .file_name(input.filename)
                .mime_str(&input.mime_type)
                .map_err(|error| error.to_string())?;
        form = form.part(file_field, part);
        self.start_upload(request.multipart(form), None, 0)
    }
}

fn upload_id(payload: &Value) -> Result<&str, String> {
    payload
        .get("uploadId")
        .and_then(Value::as_str)
        .ok_or_else(|| "上传缺少 uploadId".into())
}
