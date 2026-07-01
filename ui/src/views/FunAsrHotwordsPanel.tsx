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

export function FunAsrHotwordsPanel() {
  const providers = useProviderStore((s) => s.profiles);
  const loadProviders = useProviderStore((s) => s.load);
  const saveFunasrHotwords = useProviderStore((s) => s.saveFunasrHotwords);
  const clearFunasrHotwords = useProviderStore((s) => s.clearFunasrHotwords);
  const funasr = providers.find((p) => p.id === "funasr");

  const [hotwordsText, setHotwordsText] = useState("");
  const [vocabularyId, setVocabularyId] = useState("");
  const [message, setMessage] = useState("");

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  useEffect(() => {
    const config = funasr?.config;
    if (!config) return;
    setVocabularyId(String(config.vocabularyId ?? ""));
    const hotwords = Array.isArray(config.hotwords)
      ? (config.hotwords as { text: string; weight: number }[])
      : [];
    setHotwordsText(hotwords.map((item) => `${item.text},${item.weight}`).join("\n"));
  }, [funasr?.config]);

  const handleSave = async () => {
    const hotwords = parseHotwordsInput(hotwordsText);
    if (hotwords.length === 0) {
      setMessage("请至少输入一个热词。");
      return;
    }
    try {
      await saveFunasrHotwords(hotwords);
      setMessage("热词已保存到阿里云百炼。");
    } catch (error) {
      setMessage(`保存失败：${String(error)}`);
    }
  };

  const handleClear = async () => {
    try {
      await clearFunasrHotwords();
      setHotwordsText("");
      setMessage("热词已清除。");
    } catch (error) {
      setMessage(`清除失败：${String(error)}`);
    }
  };

  return (
    <div className="mt-4 rounded-xl border border-white/10 bg-white/[0.03] p-4">
      <Field label="Fun-ASR 热词（每行一个，格式：热词,权重；权重 1-5，可省略默认 4）">
        <Textarea
          rows={8}
          placeholder={"示例：\n说吧,4\nFun-ASR,3\n阿里云百炼,4"}
          value={hotwordsText}
          onChange={(event) => setHotwordsText(event.target.value)}
        />
      </Field>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <Button size="sm" variant="primary" onClick={handleSave} disabled={!hotwordsText.trim()}>
          保存热词到阿里云
        </Button>
        <Button size="sm" onClick={handleClear} disabled={!vocabularyId && !hotwordsText.trim()}>
          清除热词
        </Button>
        {vocabularyId && <span className="text-xs text-white/40">词表 ID：{vocabularyId}</span>}
      </div>
      {message && <p className="mt-2 text-xs text-white/50">{message}</p>}
    </div>
  );
}
