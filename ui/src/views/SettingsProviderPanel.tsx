import { useEffect, useRef, useState } from "react";
import { Collapse } from "@/components/ui/Collapse";
import { Button } from "@/components/ui/Button";
import { Field, CheckField } from "@/components/ui/Field";
import { Input } from "@/components/ui/Input";
import { Slider } from "@/components/ui/Slider";
import { CMD, cmd } from "@/lib/tauri";
import { useProviderStore } from "@/store/useProviderStore";
import { FunAsrHotwordsPanel } from "@/views/FunAsrHotwordsPanel";

const NESTED_COLLAPSE_CLASS = "border-white/10 bg-black/20";
const NESTED_HEADER_CLASS = "px-3 py-2.5";
const NESTED_BODY_CLASS = "px-3 py-3";

const API_KEY_MASK = "•".repeat(32);

function EyeIcon({ visible }: { visible: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className="h-4 w-4"
      aria-hidden
    >
      <path d="M2.1 12s3.6-6.5 9.9-6.5 9.9 6.5 9.9 6.5-3.6 6.5-9.9 6.5S2.1 12 2.1 12Z" />
      <circle cx="12" cy="12" r="2.8" />
      {!visible && <path d="M4 4 20 20" />}
    </svg>
  );
}

export function SettingsProviderPanel() {
  const providers = useProviderStore((s) => s.profiles);
  const providerStatus = useProviderStore((s) => s.statusText);
  const loadProviders = useProviderStore((s) => s.load);
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);

  const [apiKey, setApiKey] = useState("");
  const [savedApiKey, setSavedApiKey] = useState("");
  const [apiKeyVisible, setApiKeyVisible] = useState(false);
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [apiKeySaving, setApiKeySaving] = useState(false);
  const [apiKeyLoading, setApiKeyLoading] = useState(false);
  const [message, setMessage] = useState("");
  const [languageHints, setLanguageHints] = useState<string[]>([]);
  const [semanticPunctuation, setSemanticPunctuation] = useState(false);
  const [maxSentenceSilence, setMaxSentenceSilence] = useState(1300);
  const [multiThresholdMode, setMultiThresholdMode] = useState(false);
  const [heartbeat, setHeartbeat] = useState(false);
  const [noiseThreshold, setNoiseThreshold] = useState("");

  const funasr = providers.find((p) => p.id === "funasr");
  const hasApiKey = !!funasr?.status?.hasApiKey;
  const saveRequestRef = useRef(0);
  const apiKeyInputRef = useRef<HTMLInputElement>(null);
  const showStoredApiKey = !apiKey && (savedApiKey || hasApiKey);
  const apiKeyInputValue = apiKey || (showStoredApiKey ? (apiKeyVisible && savedApiKey ? savedApiKey : API_KEY_MASK) : "");

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const config = funasr?.config;
    if (!config) return;
    setLanguageHints(Array.isArray(config.languageHints) ? (config.languageHints as string[]) : []);
    setSemanticPunctuation(!!config.semanticPunctuationEnabled);
    setMaxSentenceSilence(Number(config.maxSentenceSilence ?? 1300));
    setMultiThresholdMode(!!config.multiThresholdModeEnabled);
    setHeartbeat(!!config.heartbeat);
    setNoiseThreshold(
      config.speechNoiseThreshold === null || config.speechNoiseThreshold === undefined
        ? ""
        : String(config.speechNoiseThreshold),
    );
  }, [funasr?.config]);

  useEffect(() => {
    if (!apiKeyDirty) return;
    const nextApiKey = apiKey.trim();
    if (!nextApiKey) {
      setApiKeySaving(false);
      return;
    }

    setApiKeySaving(true);
    const timer = window.setTimeout(async () => {
      const requestId = saveRequestRef.current + 1;
      saveRequestRef.current = requestId;

      try {
        await updateProviderConfig("funasr", { apiKey: nextApiKey });
        if (saveRequestRef.current !== requestId) return;
        setSavedApiKey(nextApiKey);
        setApiKeyDirty(false);
        if (document.activeElement !== apiKeyInputRef.current) {
          setApiKey("");
          setApiKeyVisible(false);
        }
        setMessage("API Key 已自动保存。");
      } catch (error) {
        if (saveRequestRef.current === requestId) setMessage(`保存失败：${String(error)}`);
      } finally {
        if (saveRequestRef.current === requestId) setApiKeySaving(false);
      }
    }, 500);

    return () => window.clearTimeout(timer);
  }, [apiKey, apiKeyDirty, updateProviderConfig]);

  const beginApiKeyEdit = () => {
    if (!apiKey && showStoredApiKey) {
      setSavedApiKey("");
      setApiKeyVisible(false);
    }
  };

  const finishApiKeyEdit = () => {
    if (!apiKey.trim() || apiKeyDirty) return;
    setApiKey("");
    setApiKeyVisible(false);
  };

  const toggleApiKeyVisibility = async () => {
    if (!apiKeyVisible && !apiKey && !savedApiKey && hasApiKey) {
      setApiKeyLoading(true);
      try {
        const realApiKey = await cmd<string>(CMD.getProviderApiKey, { providerId: "funasr" });
        setSavedApiKey(realApiKey);
        setApiKeyVisible(true);
      } catch (error) {
        setMessage(`读取 API Key 失败：${String(error)}`);
      } finally {
        setApiKeyLoading(false);
      }
      return;
    }
    setApiKeyVisible((current) => !current);
  };

  const openApiKeyPage = async () => {
    try {
      await cmd(CMD.openApiKeyPage);
    } catch (error) {
      setMessage(`打开链接失败：${String(error)}`);
    }
  };

  const toggleLanguageHint = (lang: string) => {
    setLanguageHints((prev) =>
      prev.includes(lang) ? prev.filter((value) => value !== lang) : [...prev, lang],
    );
  };

  const saveAdvanced = async () => {
    try {
      const threshold = noiseThreshold.trim();
      await updateProviderConfig("funasr", {
        languageHints,
        semanticPunctuationEnabled: semanticPunctuation,
        maxSentenceSilence,
        multiThresholdModeEnabled: multiThresholdMode,
        heartbeat,
        speechNoiseThreshold: threshold === "" ? null : Number(threshold),
      });
      setMessage("高级参数已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <Collapse
        title={funasr?.displayName || "Fun-ASR"}
        subtitle={hasApiKey ? "已配置 API Key" : "未配置 API Key"}
        defaultOpen
      >
        <p className="text-xs text-white/45">
          <button
            type="button"
            onClick={openApiKeyPage}
            className="text-[var(--color-accent-light)] underline-offset-4 hover:underline"
          >
            点击此处获取 API Key
          </button>
        </p>

        <div className="mt-3">
          <div className="relative">
            <Input
              ref={apiKeyInputRef}
              type={apiKeyVisible && (apiKey || savedApiKey) ? "text" : "password"}
              placeholder={hasApiKey ? "输入新 API Key 可覆盖当前配置" : "输入阿里云百炼 API Key"}
              value={apiKeyInputValue}
              onFocus={beginApiKeyEdit}
              onBlur={finishApiKeyEdit}
              onChange={(event) => {
                setApiKey(event.target.value);
                setApiKeyDirty(true);
                setSavedApiKey("");
              }}
              className="pr-11"
            />
            <button
              type="button"
              aria-label={apiKeyVisible ? "隐藏 API Key" : "显示 API Key"}
              onClick={toggleApiKeyVisibility}
              disabled={(!apiKey && !savedApiKey && !hasApiKey) || apiKeyLoading}
              className="absolute right-2 top-1/2 grid h-8 w-8 -translate-y-1/2 place-items-center rounded-lg text-white/45 transition-colors hover:bg-white/[0.08] hover:text-white/80 focus:outline-none focus-visible:ring-2 focus-visible:ring-[color-mix(in_srgb,var(--color-accent)_45%,transparent)] disabled:cursor-not-allowed disabled:opacity-35"
            >
              <EyeIcon visible={apiKeyVisible} />
            </button>
          </div>
        </div>
        <p className="mt-2 text-xs text-white/45">
          当前状态：{hasApiKey ? "已配置 API Key" : "未配置 API Key"}
          {apiKeySaving ? " · 正在自动保存..." : providerStatus ? ` · ${providerStatus}` : ""}
        </p>

        <div className="mt-4 flex flex-col gap-3">
          <Collapse
            title="热词"
            className={NESTED_COLLAPSE_CLASS}
            headerClassName={NESTED_HEADER_CLASS}
            bodyClassName={NESTED_BODY_CLASS}
          >
            <FunAsrHotwordsPanel />
          </Collapse>

          <Collapse
            title="高级参数"
            className={NESTED_COLLAPSE_CLASS}
            headerClassName={NESTED_HEADER_CLASS}
            bodyClassName={NESTED_BODY_CLASS}
          >
            <div>
              <p className="text-xs text-white/50">语种提示（language_hints）</p>
              <div className="mt-1.5 flex gap-4">
                {[
                  { value: "zh", label: "中文" },
                  { value: "en", label: "英文" },
                  { value: "ja", label: "日语" },
                ].map((lang) => (
                  <CheckField
                    key={lang.value}
                    checked={languageHints.includes(lang.value)}
                    onChange={() => toggleLanguageHint(lang.value)}
                  >
                    {lang.label}
                  </CheckField>
                ))}
              </div>
            </div>
            <CheckField
              className="mt-3"
              checked={semanticPunctuation}
              onChange={setSemanticPunctuation}
            >
              语义断句（semantic_punctuation_enabled）
            </CheckField>
            <div className="mt-3">
              <Slider
                label="断句静音阈值"
                min={200}
                max={6000}
                step={100}
                value={maxSentenceSilence}
                format={(value) => `${value.toFixed(0)} ms`}
                onChange={setMaxSentenceSilence}
              />
            </div>
            <CheckField
              className="mt-3"
              checked={multiThresholdMode}
              onChange={setMultiThresholdMode}
              disabled={semanticPunctuation}
            >
              多阈值模式（multi_threshold_mode_enabled，防止 VAD 断句切割过长，仅在语义断句关闭时生效）
            </CheckField>
            <CheckField className="mt-3" checked={heartbeat} onChange={setHeartbeat}>
              心跳包（heartbeat，长时间静音保活连接）
            </CheckField>
            <Field label="噪音判定阈值（speech_noise_threshold，-1.0 ~ 1.0，留空使用默认）" className="mt-3">
              <Input
                type="number"
                min={-1}
                max={1}
                step={0.1}
                value={noiseThreshold}
                onChange={(event) => setNoiseThreshold(event.target.value)}
              />
            </Field>
            <Button size="sm" className="mt-3" onClick={saveAdvanced}>
              保存高级参数
            </Button>
          </Collapse>
        </div>

        {message && <p className="mt-3 text-xs text-white/50">{message}</p>}
      </Collapse>
    </div>
  );
}
