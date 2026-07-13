import { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Field } from "@/components/ui/Field";
import { Textarea } from "@/components/ui/Input";
import { useProviderStore } from "@/store/useProviderStore";

function parseHotwordsInput(text: string): { text: string; weight: number }[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const [rawText, rawWeight] = line.split(",").map((part) => part.trim());
      const weight = Number(rawWeight);
      return {
        text: rawText ?? "",
        weight: Number.isFinite(weight) && weight >= 1 && weight <= 5 ? weight : 4,
      };
    })
    .filter((item) => item.text.length > 0);
}

export function FunAsrHotwordsPanel({ providerId = "funasr" }: { providerId?: string }) {
  const providers = useProviderStore((s) => s.profiles);
  const loadProviders = useProviderStore((s) => s.load);
  const saveHotwords = useProviderStore((s) => s.saveHotwords);
  const syncHotwords = useProviderStore((s) => s.syncHotwords);
  const clearHotwords = useProviderStore((s) => s.clearHotwords);
  const provider = providers.find((p) => p.id === providerId);

  const [hotwordsText, setHotwordsText] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const config = provider?.config;
    if (!config) return;
    const hotwords = Array.isArray(config.hotwords)
      ? (config.hotwords as { text: string; weight: number }[])
      : [];
    setHotwordsText(hotwords.map((item) => `${item.text},${item.weight}`).join("\n"));
  }, [provider?.config]);

  const handleSave = async () => {
    const hotwords = parseHotwordsInput(hotwordsText);
    if (hotwords.length === 0) {
      setMessage("请至少输入一个热词。");
      return;
    }
    try {
      await saveHotwords(providerId, hotwords);
      setMessage("热词已保存。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  const handleSync = async () => {
    try {
      await syncHotwords(providerId);
      setMessage("热词已从供应商同步。");
    } catch (error) {
      setMessage(`同步失败：${String(error)}`);
    }
  };

  const handleClear = async () => {
    try {
      await clearHotwords(providerId);
      setHotwordsText("");
      setMessage("热词已清除。");
    } catch (error) {
      setMessage(`清除失败：${String(error)}`);
    }
  };

  return (
    <>
      <Field label="热词（每行一个，格式：热词,权重；权重 1-5，可省略默认 4）">
        <Textarea
          rows={8}
          placeholder={"示例：\n说吧,4\nFun-ASR,3\n阿里云百炼,4"}
          value={hotwordsText}
          onChange={(event) => setHotwordsText(event.target.value)}
        />
      </Field>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" variant="primary" onClick={handleSave} disabled={!hotwordsText.trim()}>
          保存热词
        </Button>
        <Button size="sm" onClick={handleSync}>
          从云端同步
        </Button>
        <Button size="sm" onClick={handleClear} disabled={!hotwordsText.trim()}>
          清除热词
        </Button>
      </div>
      {message && <p className="mt-2 text-xs text-[var(--color-fg-subtle)]">{message}</p>}
    </>
  );
}
