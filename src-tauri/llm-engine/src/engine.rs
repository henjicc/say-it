//! llama.cpp 上的单会话文本改写：静态提示词与已口述的文字留检查点，每次只算新增部分；
//! 生成时用草稿批量验证（见 `draft`）。贪心解码；批量与逐个计算有浮点误差，极少数势均力敌的位置可能选到另一个 token。

use crate::draft::{propose, Pointer};
use llama_cpp_sys_2 as sys;
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::time::Instant;

/// 只用一个序列：循环状态的回滚快照按序列分配，多一个序列就多占一份显存。
const WORK: i32 = 0;
/// 两个检查点：提示词里听写开始就确定的部分算完时，和已口述文字算完时。
const HEAD: usize = 0;
const BODY: usize = 1;
/// 检查点离字符串末尾留出的 token 数：拼上后续文字时，交界处的分词可能变化。
const HEAD_MARGIN: usize = 2;
const BODY_MARGIN: usize = 3;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoadRequest {
    pub(crate) model: String,
    pub(crate) gpu: bool,
    pub(crate) threads: i32,
    pub(crate) context_tokens: u32,
    /// 一次前向最多验证的草稿 token 数。
    pub(crate) draft_tokens: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunRequest {
    /// 提示词里听写开始时就确定的部分。
    pub(crate) head: String,
    /// 随口述增长的部分。
    pub(crate) body: String,
    /// 收尾部分，直到模型开始输出的位置。
    pub(crate) tail: String,
    /// 口述原文，输出多半照抄它。
    pub(crate) source: String,
    /// 同一段话上一版的整理结果，没有时为空。
    pub(crate) previous: String,
    pub(crate) max_tokens: usize,
    /// 边说边做的预先整理：正式请求一到就让路。
    pub(crate) speculative: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunOutcome {
    pub(crate) text: String,
    pub(crate) cancelled: bool,
    pub(crate) prompt_tokens: usize,
    /// 提示词里这次真正计算的 token 数，其余来自检查点。
    pub(crate) prompt_evaluated: usize,
    pub(crate) output_tokens: usize,
    /// 生成阶段的前向次数；草稿全中时远小于输出 token 数。
    pub(crate) forward_passes: usize,
    pub(crate) prompt_ms: f64,
    pub(crate) generate_ms: f64,
}

pub(crate) struct Engine {
    model: *mut sys::llama_model,
    context: *mut sys::llama_context,
    vocab: *const sys::llama_vocab,
    sampler: *mut sys::llama_sampler,
    vocab_size: usize,
    batch: sys::llama_batch,
    context_tokens: usize,
    draft_tokens: usize,
    on_gpu: bool,
    checkpoints: [Checkpoint; 2],
}

/// 某段提示词算完时的模型状态，存在内存里。
#[derive(Default)]
struct Checkpoint {
    tokens: Vec<i32>,
    state: Vec<u8>,
}

unsafe extern "C" fn log_warnings(
    level: sys::ggml_log_level,
    text: *const std::os::raw::c_char,
    _: *mut std::os::raw::c_void,
) {
    if level == sys::GGML_LOG_LEVEL_WARN || level == sys::GGML_LOG_LEVEL_ERROR {
        eprint!("{}", std::ffi::CStr::from_ptr(text).to_string_lossy());
    }
}

/// 权重是内存映射读进来的，传到显卡之后这些页还挂在进程名下，任务管理器里看着像占了几个 GB。
/// 让系统把暂时不用的页收回去；之后真用到的页会再读进来。模型在处理器上时权重就是工作内存，不做。
fn release_load_pages() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn SetProcessWorkingSetSize(process: *mut std::ffi::c_void, min: usize, max: usize) -> i32;
    }
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

fn common_prefix(a: &[i32], b: &[i32]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

impl Engine {
    /// 模型是否跑在显卡上。
    pub(crate) fn on_gpu(&self) -> bool {
        self.on_gpu
    }

    pub(crate) fn load(request: &LoadRequest) -> Result<Self, String> {
        let path = CString::new(request.model.as_str()).map_err(|_| "模型路径含空字符")?;
        unsafe {
            sys::llama_log_set(Some(log_warnings), std::ptr::null_mut());
            sys::llama_backend_init();
            let on_gpu = request.gpu && sys::llama_supports_gpu_offload();
            let mut model_params = sys::llama_model_default_params();
            model_params.n_gpu_layers = if on_gpu { 999 } else { 0 };
            let model = sys::llama_model_load_from_file(path.as_ptr(), model_params);
            if model.is_null() {
                return Err(format!("加载模型失败：{}", request.model));
            }
            let mut params = sys::llama_context_default_params();
            params.n_ctx = request.context_tokens;
            params.n_batch = request.context_tokens;
            params.n_seq_max = 1;
            // 混合结构的模型带循环状态，回滚被否决的草稿靠逐 token 快照，数量就是草稿上限。
            params.n_rs_seq = request.draft_tokens;
            params.n_threads = request.threads;
            params.n_threads_batch = request.threads;
            params.no_perf = true;
            // 取最大值放在显卡上做：每个位置只读回一个 token，不用把整张词表的分数拷回内存。
            let sampler = sys::llama_sampler_chain_init(sys::llama_sampler_chain_default_params());
            sys::llama_sampler_chain_add(sampler, sys::llama_sampler_init_greedy());
            let mut sampler_config = sys::llama_sampler_seq_config {
                seq_id: WORK,
                sampler,
            };
            params.samplers = &mut sampler_config;
            params.n_samplers = 1;
            params.n_outputs_max_per_seq = request.draft_tokens + 1;
            // 不设的话后端按「整批每个位置都要输出」预留缓冲，白占近 1 GB 显存。
            params.n_outputs_max = request.draft_tokens + 1;
            let context = sys::llama_init_from_model(model, params);
            if context.is_null() {
                sys::llama_model_free(model);
                return Err("创建推理上下文失败".into());
            }
            let vocab = sys::llama_model_get_vocab(model);
            let mut engine = Self {
                model,
                context,
                vocab,
                sampler,
                vocab_size: sys::llama_vocab_n_tokens(vocab) as usize,
                batch: sys::llama_batch_init(request.context_tokens as i32, 0, 1),
                context_tokens: request.context_tokens as usize,
                draft_tokens: request.draft_tokens as usize,
                on_gpu,
                checkpoints: Default::default(),
            };
            engine.warm_up()?;
            if on_gpu {
                release_load_pages();
            }
            Ok(engine)
        }
    }

    fn warm_up(&mut self) -> Result<(), String> {
        let filler = self.tokenize("。");
        // 后端第一次遇到某个输出位置数要花几百毫秒做准备，生成阶段只用这两种，加载时各跑一遍。
        for size in [1, 1 + self.draft_tokens] {
            self.decode(&vec![filler[0]; size], 0, true)?;
            unsafe { sys::llama_memory_clear(sys::llama_get_memory(self.context), true) };
        }
        Ok(())
    }

    fn tokenize(&self, text: &str) -> Vec<i32> {
        let mut tokens = vec![0i32; text.len() + 8];
        let count = unsafe {
            sys::llama_tokenize(
                self.vocab,
                text.as_ptr().cast(),
                text.len() as i32,
                tokens.as_mut_ptr(),
                tokens.len() as i32,
                false,
                true,
            )
        };
        // 每个 token 至少对应一个字节，缓冲区一定够。
        tokens.truncate(count.max(0) as usize);
        tokens
    }

    fn append_piece(&self, token: i32, bytes: &mut Vec<u8>) {
        let mut buffer = vec![0u8; 64];
        let piece = |buffer: &mut Vec<u8>| unsafe {
            sys::llama_token_to_piece(
                self.vocab,
                token,
                buffer.as_mut_ptr().cast(),
                buffer.len() as i32,
                0,
                false,
            )
        };
        let mut length = piece(&mut buffer);
        // 缓冲区不够时返回所需长度的相反数。
        if length < 0 {
            buffer.resize(length.unsigned_abs() as usize, 0);
            length = piece(&mut buffer);
        }
        bytes.extend_from_slice(&buffer[..length.max(0) as usize]);
    }

    /// 把 `tokens` 接在工作序列的 `position` 之后算一遍；`all_logits` 为假时只取最后一个位置的输出。
    fn decode(&mut self, tokens: &[i32], position: usize, all_logits: bool) -> Result<(), String> {
        unsafe {
            for (index, token) in tokens.iter().enumerate() {
                *self.batch.token.add(index) = *token;
                *self.batch.pos.add(index) = (position + index) as i32;
                *self.batch.n_seq_id.add(index) = 1;
                *(*self.batch.seq_id.add(index)) = WORK;
                *self.batch.logits.add(index) = (all_logits || index + 1 == tokens.len()) as i8;
            }
            self.batch.n_tokens = tokens.len() as i32;
            match sys::llama_decode(self.context, self.batch) {
                0 => Ok(()),
                code => Err(format!("推理失败（llama_decode 返回 {code}）")),
            }
        }
    }

    /// 上一次前向里第 `index` 个位置给出的下一个 token；-1 表示最后一个位置。
    fn argmax(&self, index: i32) -> i32 {
        let sampled = unsafe { sys::llama_get_sampled_token_ith(self.context, index) };
        if sampled >= 0 {
            return sampled;
        }
        // 后端不支持在显卡上采样时退回读取整张词表的分数。
        let logits = unsafe {
            std::slice::from_raw_parts(
                sys::llama_get_logits_ith(self.context, index),
                self.vocab_size,
            )
        };
        let mut best = 0;
        for (token, value) in logits.iter().enumerate() {
            if *value > logits[best] {
                best = token;
            }
        }
        best as i32
    }

    fn save_checkpoint(&mut self, slot: usize, tokens: &[i32]) {
        let mut state = vec![0u8; unsafe { sys::llama_state_seq_get_size(self.context, WORK) }];
        let written = unsafe {
            sys::llama_state_seq_get_data(self.context, state.as_mut_ptr(), state.len(), WORK)
        };
        state.truncate(written);
        self.checkpoints[slot] = Checkpoint {
            tokens: tokens.to_vec(),
            state,
        };
    }

    /// 让模型回到与 `prompt` 前缀一致的最长检查点，返回已有的 token 数。
    fn restore(&mut self, prompt: &[i32]) -> usize {
        unsafe { sys::llama_memory_clear(sys::llama_get_memory(self.context), false) };
        for slot in [BODY, HEAD] {
            let saved = &self.checkpoints[slot];
            if saved.state.is_empty() || !prompt.starts_with(&saved.tokens) {
                continue;
            }
            let read = unsafe {
                sys::llama_state_seq_set_data(
                    self.context,
                    saved.state.as_ptr(),
                    saved.state.len(),
                    WORK,
                )
            };
            if read > 0 {
                return saved.tokens.len();
            }
        }
        0
    }

    pub(crate) fn run(
        &mut self,
        request: &RunRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RunOutcome, String> {
        let started = Instant::now();
        let head_and_body = format!("{}{}", request.head, request.body);
        let prompt = self.tokenize(&format!("{head_and_body}{}", request.tail));
        if prompt.len() + request.max_tokens + self.draft_tokens + 1 > self.context_tokens {
            return Err("文本太长，超出本地模型的处理上限".into());
        }
        let head_end =
            common_prefix(&prompt, &self.tokenize(&request.head)).saturating_sub(HEAD_MARGIN);
        let body_end = common_prefix(&prompt, &self.tokenize(&head_and_body))
            .saturating_sub(BODY_MARGIN)
            .max(head_end);

        let restored = self.restore(&prompt);
        let mut position = restored;
        for (slot, end) in [
            (Some(HEAD), head_end),
            (Some(BODY), body_end),
            (None, prompt.len()),
        ] {
            if end > position {
                if cancelled() {
                    return Ok(RunOutcome {
                        cancelled: true,
                        ..Default::default()
                    });
                }
                self.decode(&prompt[position..end], position, false)?;
                position = end;
            }
            // 恢复点可能比这个检查点更靠后（原文末尾的分词变了），此时模型状态不在 `end`，不能存。
            if let Some(slot) = slot {
                if end > 0 && position == end && self.checkpoints[slot].tokens != prompt[..end] {
                    self.save_checkpoint(slot, &prompt[..end]);
                }
            }
        }
        let mut outcome = RunOutcome {
            prompt_tokens: prompt.len(),
            prompt_evaluated: prompt.len() - restored,
            prompt_ms: started.elapsed().as_secs_f64() * 1000.0,
            ..Default::default()
        };
        let generation_started = Instant::now();
        let source_tokens = self.tokenize(&request.source);
        let previous_tokens = self.tokenize(&request.previous);
        let mut source = Pointer::new(&source_tokens);
        let mut previous = Pointer::new(&previous_tokens);
        let mut output: Vec<i32> = Vec::new();
        // 检查点都留了余量，提示词的最后几个 token 每次都会重新计算，这里一定有输出可读。
        let mut next = self.argmax(-1);
        let mut step: Vec<i32> = Vec::with_capacity(self.draft_tokens + 1);
        loop {
            let finished = unsafe { sys::llama_vocab_is_eog(self.vocab, next) };
            if finished || output.len() >= request.max_tokens {
                break;
            }
            if cancelled() {
                outcome.cancelled = true;
                break;
            }
            output.push(next);
            source.follow(next);
            previous.follow(next);
            let room = request.max_tokens - output.len();
            let proposal = propose(&previous, &source);
            // 照抄原文的草稿平均三四个 token 就被标点或改字打断。显卡上多验证几个位置几乎不要钱；
            // 处理器上每个位置都要实打实地算，猜不中就是白算，所以只在照抄上一版结果时才猜。
            let limit = if proposal.from_previous || self.on_gpu {
                self.draft_tokens
            } else {
                0
            };
            let draft = &proposal.tokens[..proposal.tokens.len().min(limit).min(room)];
            step.clear();
            step.push(next);
            step.extend_from_slice(draft);
            // 有草稿就补齐到固定长度，多出的位置算完一并回滚：输出位置数只有两种，见 `warm_up`。
            if step.len() > 1 {
                step.resize(1 + self.draft_tokens, next);
            }
            let step_position = prompt.len() + output.len() - 1;
            self.decode(&step, step_position, true)?;
            outcome.forward_passes += 1;

            let mut accepted = 0;
            next = self.argmax(0);
            while accepted < draft.len() && draft[accepted] == next {
                output.push(next);
                source.follow(next);
                previous.follow(next);
                accepted += 1;
                next = self.argmax(accepted as i32);
            }
            if 1 + accepted < step.len() {
                let keep = (step_position + 1 + accepted) as i32;
                let memory = unsafe { sys::llama_get_memory(self.context) };
                if !unsafe { sys::llama_memory_seq_rm(memory, WORK, keep, -1) } {
                    return Err("回滚草稿失败".into());
                }
            }
        }
        let mut bytes = Vec::new();
        for token in &output {
            self.append_piece(*token, &mut bytes);
        }
        outcome.text = String::from_utf8_lossy(&bytes).into_owned();
        outcome.output_tokens = output.len();
        outcome.generate_ms = generation_started.elapsed().as_secs_f64() * 1000.0;
        Ok(outcome)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            sys::llama_batch_free(self.batch);
            sys::llama_free(self.context);
            sys::llama_sampler_free(self.sampler);
            sys::llama_model_free(self.model);
        }
    }
}
