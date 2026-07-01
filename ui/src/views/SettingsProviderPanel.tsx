import { useEffect, useState } from "react";
import { Card, CardTitle, CardDescription } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Field, CheckField } from "@/components/ui/Field";
import { Input } from "@/components/ui/Input";
import { Slider } from "@/components/ui/Slider";
import { useProviderStore } from "@/store/useProviderStore";

export function SettingsProviderPanel() {
  const providers = useProviderStore((s) => s.profiles);
  const providerStatus = useProviderStore((s) => s.statusText);
  const loadProviders = useProviderStore((s) => s.load);
  const updateProviderConfig = useProviderStore((s) => s.updateConfig);

  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState("");
  const [languageHints, setLanguageHints] = useState<string[]>([]);
  const [semanticPunctuation, setSemanticPunctuation] = useState(false);
  const [maxSentenceSilence, setMaxSentenceSilence] = useState(1300);
  const [heartbeat, setHeartbeat] = useState(false);
  const [noiseThreshold, setNoiseThreshold] = useState("");

  const funasr = providers.find((p) => p.id === "funasr");
  const hasApiKey = !!funasr?.status?.hasApiKey;

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const config = funasr?.config;
    if (!config) return;
    setLanguageHints(Array.isArray(config.languageHints) ? (config.languageHints as string[]) : []);
    setSemanticPunctuation(!!config.semanticPunctuationEnabled);
    setMaxSentenceSilence(Number(config.maxSentenceSilence ?? 1300));
    setHeartbeat(!!config.heartbeat);
    setNoiseThreshold(
      config.speechNoiseThreshold === null || config.speechNoiseThreshold === undefined
        ? ""
        : String(config.speechNoiseThreshold),
    );
  }, [funasr?.config]);

  const saveApiKey = async () => {
    try {
      await updateProviderConfig("funasr", { apiKey });
      setApiKey("");
      setMessage("API Key 已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
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
        heartbeat,
        speechNoiseThreshold: threshold === "" ? null : Number(threshold),
      });
      setMessage("高级参数已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  return (
    <Card>
      <CardTitle>Fun-ASR</CardTitle>
      <CardDescription>
        说吧！v1 只启用 Fun-ASR。API Key 保存在本机 Tauri 数据目录中，不会在前端回显。
      </CardDescription>

      <div className="mt-4 flex items-center gap-2">
        <Input
          type="password"
          placeholder={hasApiKey ? "已保存 API Key，输入新值可覆盖" : "输入阿里云百炼 API Key"}
          value={apiKey}
          onChange={(event) => setApiKey(event.target.value)}
        />
        <Button variant="primary" onClick={saveApiKey} disabled={!apiKey.trim()}>
          保存
        </Button>
      </div>
      <p className="mt-2 text-xs text-white/45">
        当前状态：{hasApiKey ? "已配置 API Key" : "未配置 API Key"}
        {providerStatus ? ` · ${providerStatus}` : ""}
      </p>

      <div className="mt-5 rounded-lg border border-white/10 bg-black/20 p-3">
        <p className="text-sm font-medium text-white/80">识别高级参数</p>
        <div className="mt-3">
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
      </div>

      {message && <p className="mt-3 text-xs text-white/50">{message}</p>}
    </Card>
  );
}
