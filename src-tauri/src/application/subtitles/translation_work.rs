use super::translation_queue::Ready;
use super::*;

pub(super) struct Job {
    text: String,
    prefs: crate::application::translation::TranslationPrefs,
    source: String,
    target: String,
}

pub(super) fn enqueue(app: AppHandle, epoch: u64, seq: u64, text: String) {
    let prepared = (|| -> Result<Vec<Ready<Job>>, String> {
        let state = app.state::<RuntimeState>();
        let mut session = state
            .subtitle_runtime
            .session
            .lock()
            .map_err(|_| "字幕状态锁失败")?;
        // 分句从 ASR 事件携带 epoch，不在稍后排队时重新读取新会话编号。
        if session.epoch != epoch
            || !session.prefs.translation_enabled()
            || session.translation_cancellation.is_cancelled()
            || !session.translation.values.contains_key(&seq)
            || matches!(session.phase, SubtitlePhase::Idle | SubtitlePhase::Stopping)
        {
            return Ok(Vec::new());
        }
        session.cancel_obsolete_translations();
        let prefs = crate::application::translation::current(&state)?;
        crate::application::translation::validate(&state, &prefs)?;
        let bytes = text.len();
        let job = Job {
            text,
            prefs,
            source: session.prefs.translation_source_lang.clone(),
            target: session.prefs.translation_target_lang.clone(),
        };
        let cancellation = session.translation_cancellation.clone();
        session
            .translation_jobs
            .enqueue(seq, bytes, job, &cancellation)
    })();
    match prepared {
        Ok(ready) => {
            for task in ready {
                spawn(app.clone(), epoch, task);
            }
        }
        // 容量或配置错误直接进入已有字幕错误投影，不能依赖可能落后的广播消费。
        Err(error) => handle_translation(&app, epoch, seq, "", true, Some(&error)),
    }
}

struct Completion {
    app: AppHandle,
    epoch: u64,
    seq: u64,
    cancellation: CancellationToken,
}

impl Drop for Completion {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let ready = {
            let state = self.app.state::<RuntimeState>();
            let Ok(mut session) = state.subtitle_runtime.session.lock() else {
                return;
            };
            if session.epoch != self.epoch {
                return;
            }
            let cancellation = session.translation_cancellation.clone();
            session.translation_jobs.finish(self.seq, &cancellation)
        };
        // 在锁外启动后续请求；即使 UI/广播事件延迟，工作名额也能正确释放。
        for task in ready {
            spawn(self.app.clone(), self.epoch, task);
        }
    }
}

fn spawn(app: AppHandle, epoch: u64, task: Ready<Job>) {
    let completion = Completion {
        app: app.clone(),
        epoch,
        seq: task.seq,
        cancellation: task.cancellation.clone(),
    };
    tauri::async_runtime::spawn(async move {
        let _completion = completion;
        if task.cancellation.is_cancelled() {
            return;
        }
        let delta_app = app.clone();
        let delta_cancellation = task.cancellation.clone();
        let seq = task.seq;
        // 执行时再解析供应商：排队期间插件可能被停用或卸载，不能沿用入队时的执行权限。
        let result = crate::application::translation::translate_with(
            &app.state::<RuntimeState>(),
            &task.job.prefs,
            &task.job.text,
            &task.job.source,
            &task.job.target,
            task.cancellation.clone(),
            move |partial| {
                if !delta_cancellation.is_cancelled() {
                    handle_translation(&delta_app, epoch, seq, partial, false, None);
                }
            },
        )
        .await;
        if task.cancellation.is_cancelled() {
            return;
        }
        let (text, error) = match result {
            Ok(text) => (text, None),
            Err(error) => (String::new(), Some(error)),
        };
        handle_translation(&app, epoch, seq, &text, true, error.as_deref());
    });
}
