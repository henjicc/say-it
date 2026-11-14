"""按 docs/task/本地模型多任务微调/01-训练数据规范.md 合成本地整理模型的意图、编辑、翻译、查词数据。

用法（在仓库根目录）：
  python3 -I scripts/synth-intent-data.py run [任务名前缀 ...]   合成，可中断后续跑；不给前缀时按顺序跑全部任务
  python3 -I scripts/synth-intent-data.py stats                  各任务已保留 / 已丢弃条数
  python3 -I scripts/synth-intent-data.py pack                   整理成训练集、测试集并打包

环境变量：SYNTH_BASE、SYNTH_KEY（OpenAI 兼容接口），SYNTH_MODEL（默认 workbuddy/deepseek-v4.1-flash），SYNTH_CONC（并发批数，默认 6），
SYNTH_SCALE（配额倍数，试跑时用小于 1 的值）。
产物在 工作区/intent-data/（已 gitignore）：raw/ 是每条样本连同复核、质检结果的原始记录，pack 后 final/ 是按规范格式整理的数据。
"""
import json, os, random, re, sys, tarfile, threading, time, urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "工作区" / "intent-data"
MODEL = os.environ.get("SYNTH_MODEL", "workbuddy/deepseek-v4.1-flash")
CONC = int(os.environ.get("SYNTH_CONC", "6"))
PER_CALL = 6

# ---------- 接口 ----------

def chat(system, user, effort="low", temperature=None):
    payload = {"model": MODEL, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}], "reasoning_effort": effort}
    if temperature is not None:
        payload["temperature"] = temperature
    body = json.dumps(payload).encode()
    for attempt in range(4):
        try:
            request = urllib.request.Request(os.environ["SYNTH_BASE"].rstrip("/") + "/v1/chat/completions", body,
                                             {"Authorization": "Bearer " + os.environ["SYNTH_KEY"], "Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=300) as response:
                return json.load(response)["choices"][0]["message"]["content"].strip()
        except Exception as error:  # 网关偶发超时或限流，退避重试
            last = error
            time.sleep(5 * (attempt + 1))
    raise last


def parse_json(text):
    text = re.sub(r"^```(?:json)?\s*|\s*```$", "", text.strip())
    start = min(i for i in (text.find("["), text.find("{")) if i >= 0)
    return json.loads(text[start:])

# ---------- 分布 ----------

SCENE_APPS = {
    "AI 对话": (["Claude", "ChatGPT", "Cursor", "豆包", "Kimi", "DeepSeek"], 25),
    "聊天": (["微信", "飞书", "Slack", "QQ", "钉钉"], 25),
    "写作": (["Word", "备忘录", "Notion", "邮件", "WPS", "Pages"], 20),
    "浏览器": (["Chrome", "Safari", "Edge"], 10),
    "代码编辑器": (["VS Code", "Cursor", "IntelliJ IDEA"], 8),
    "终端": (["终端", "iTerm"], 2),
    "通用": (["预览", "访达", "PDF 阅读器", "桌面"], 10),
}
NOISE = [("干净", 30), ("轻度", 50), ("重度", 20)]
SEL_STYLE = [("只有很短的指令", 40), ("指令加一句补充要求", 25), ("先嘀咕一句背景再给指令", 20), ("指令中途改口", 15)]
SPEECH_TRANSLATE_STYLE = [("指令在前", 30), ("指令在后", 30), ("指令夹在中间", 20), ("说完内容再补一句要翻译", 20)]
TARGET_LANGS = [("英文", 70), ("日文", 9), ("韩文", 6), ("法文", 5), ("德文", 5), ("西班牙文", 5)]
SEL_LANG = [("中文", 55), ("英文", 35), ("日文或韩文", 5), ("中英混排或代码", 5)]
LOCAL_EDITS = ["纠错", "加标点分段", "改正式", "缩短", "整理成列表"]
OTHER_EDITS = ["扩写", "续写", "改成邮件", "换语气（更客气或更轻松）", "总结", "给代码加注释", "改成一句话标题"]


def pick(rng, items):
    return rng.choices([v for v, _ in items], weights=[w for _, w in items])[0]


def pick_scene(rng, allowed):
    names = [s for s in SCENE_APPS if s in allowed]
    return rng.choices(names, weights=[SCENE_APPS[s][1] for s in names])[0]

# ---------- 判定规则（与规范「判定规则」一致，标签只从这里推出） ----------
# kind：content 新内容 / request 对 AI 或他人的请求 / chat_question 问对方 / speech_translate 要求翻译口述 /
#       meaning 想知道词义 / lang_rewrite 把词换成某语言写法 / translate_sel 翻译选中句段 / edit 修改选中 / question 对选中或软件提问

def derive_label(editable, sel_kind, kind):
    if sel_kind is None:
        if not editable:
            return "ask"
        return "translate" if kind == "speech_translate" else "dictate"
    if kind == "content":
        return "dictate" if editable else "ask"
    if kind == "meaning":
        return "define" if sel_kind == "word" else "ask"
    if kind == "lang_rewrite":
        return "translate"
    return {"translate_sel": "translate", "edit": "edit", "question": "ask"}[kind]

# ---------- 类别 ----------
# sel：选中文字的写法说明（None 表示没有选中）；sel_kind：None / word / text；editable：True / False / 比例

CATEGORIES = {
    "dictate.ai_imperative": dict(kind="request", sel=None, editable=True, scenes=["AI 对话", "聊天"],
        desc="用户在输入框里听写一段要发给 AI 助手或同事的话，里面有祈使句或请求（「你帮我看下日志」「帮我总结一下这篇文章」「去跑一次构建」「把这段翻译成英文发给他」），但这段话本身就是要发出去的内容，不是对本软件的指令。"),
    "dictate.task_words": dict(kind="content", sel=None, editable=True, scenes=["聊天", "写作", "AI 对话"],
        desc="用户听写的内容里提到了翻译、修改、总结、查词这类词，但只是在说事情（「这段翻译得不对」「明天记得翻译合同」「她说要改正式一点」「你把那个词查一下发我」），不是要本软件去做。"),
    "dictate.replace": dict(kind="content", sel="用户刚在输入框里写下的一段话（1～4 句，中文为主）", sel_kind="text", editable=True, scenes=["聊天", "写作", "AI 对话"],
        relations=["选中的是上一次听写识别或整理出错的结果（要有明显的错字、断句或整理错误），用户把同一件事重说一遍",
                   "用户改了主意，说的意思和选中的不同", "用户换个说法并补充了新信息"],
        desc="用户选中了自己刚输入的一段文字，直接说一段新内容来替换它。口述读起来要像一段能直接发出去的内容，不能出现「改一下」「换个方向」「别写……了」「把这段」这类针对旧文字的说法，也不提及选中的文字。"),
    "dictate.unrelated_sel": dict(kind="content", sel="输入框里碰巧被选中的一段文字（可能是别人的消息、模板里的占位文字或之前写的内容）", sel_kind="text", editable=True, scenes=["聊天", "写作", "AI 对话"],
        desc="用户没注意到有文字被选中，直接听写一段与选中文字无关的新内容。"),
    "dictate.chat_question": dict(kind="chat_question", sel=None, editable=True, scenes=["聊天", "AI 对话"],
        desc="用户在聊天或 AI 对话的输入框里听写一个问句，问的是聊天对象或那个 AI（「你吃饭了吗」「这个接口怎么调啊」），不是问本软件。"),
    "edit": dict(kind="edit", sel="一段需要被修改的文字，语言按设定；长度从一句到三段", sel_kind="text", editable=0.85, scenes=["写作", "聊天", "AI 对话", "浏览器", "代码编辑器"],
        desc="用户选中一段文字，口头要求按设定的子任务修改它。选中文字要真的有可改之处（错别字、没标点、太口语、太长、几件事挤在一段、代码没注释等）。"),
    "translate.speech": dict(kind="speech_translate", sel=None, editable=True, scenes=["聊天", "写作", "AI 对话"],
        desc="用户没有选中文字，口述一段内容并明确要求翻译成设定的目标语言再输入（「翻成英文」「用日语说」「英文说一下」）。内容是聊天或工作里真实会说的话。"),
    "translate.selection": dict(kind="translate_sel", sel="一个句子或一段话（不是单词）。焦点可输入时多为用户自己写的中文；不可输入时多为网页、文档、聊天记录里的外文", sel_kind="text", editable=0.5, scenes=["浏览器", "写作", "聊天", "通用"],
        desc="用户选中句子或段落，口头要求翻译（「翻译一下」「翻成中文」「这段翻成英文」），可以不说目标语言。"),
    "define": dict(kind="meaning", sel="词级内容（单个词、固定短语、缩写、成语或俗语，没有句末标点）", sel_kind="word", editable=0.4, scenes=["浏览器", "写作", "通用", "聊天", "代码编辑器"],
        desc="用户选中一个词或短语，想知道它本身的意思（「翻译一下」「这个词什么意思」「是什么」「啥意思」「查一下」「解释下这个词」）。屏幕文字里要有一段包含这个词的上下文，能看出它在这里的意思。"),
    "ask.selection": dict(kind="question", sel="一段文字、代码、公式、报错或条款", sel_kind="text", editable=0.4, scenes=["浏览器", "代码编辑器", "写作", "通用", "终端"],
        desc="用户选中一段内容，向软件提问关于它的问题（什么意思、对不对、有什么问题、为什么这么写、里面某个词在这里指什么），要的是解答，不是修改它。"),
    "ask.no_input": dict(kind="question", sel=None, editable=False, scenes=["通用", "浏览器"],
        desc="当前焦点不是输入框（桌面、访达、阅读器、网页正文），用户没有选中文字，直接问软件一个问题（常识、换算、怎么操作电脑）；约一成是「翻译一下 xxx」这种让软件翻译一句话。"),
}
DEFINE_TYPES = [("英文单词或短语", 45), ("中文词或网络用语", 22), ("成语或俗语", 8), ("缩写或专有名词", 15), ("生造词、很新的产品名或拼错的词（释义应为找不到）", 10)]


def needs_output(sample):
    label = sample["label"]
    if label == "dictate" or label == "define":
        return True
    if label == "edit":
        return sample["subtask"] in LOCAL_EDITS
    if label == "translate":
        source = sample["selection"] or sample["spoken"]
        return len(source) <= 200
    return False

# ---------- 任务清单 ----------
# 每个任务按配额保留到 count 条；batch 是样本首次用于哪一批训练

SCALE = float(os.environ.get("SYNTH_SCALE", "1"))


def job(name, category, split, batch, count, **frame):
    return dict(name=name, category=category, split=split, batch=batch, count=max(1, round(count * SCALE)), frame=frame)

JOBS = []
for split, scale in (("train", 1.0), ("test", 0.115)):
    for category, count in (("dictate.ai_imperative", 400), ("dictate.task_words", 200), ("dictate.replace", 500), ("dictate.unrelated_sel", 150),
                            ("dictate.chat_question", 250), ("edit", 700), ("translate.speech", 300), ("translate.selection", 300),
                            ("define", 300), ("ask.selection", 350), ("ask.no_input", 150)):
        JOBS.append(job(f"b1-{split}-{category}", category, split, 1, max(20, round(count * scale))))
    JOBS.append(job(f"b1-{split}-contrast", "contrast", split, 1, 50 if split == "train" else 25))
for subtask in LOCAL_EDITS:
    JOBS.append(job(f"b2-train-edit-{subtask}", "edit", "train", 2, 450, subtask=subtask))
    JOBS.append(job(f"b2-test-edit-{subtask}", "edit", "test", 2, 60, subtask=subtask))
JOBS += [
    job("b3-train-translate.speech", "translate.speech", "train", 3, 700),
    job("b3-train-translate.selection", "translate.selection", "train", 3, 800, short=True),
    job("b3-train-define", "define", "train", 3, 1500),
    job("b3-test-translate.speech", "translate.speech", "test", 3, 90),
    job("b3-test-translate.selection", "translate.selection", "test", 3, 110, short=True),
    job("b3-test-define", "define", "test", 3, 150, define_type="常用"),
    job("b3-test-define-rare", "define", "test", 3, 50, define_type="生造词、很新的产品名或拼错的词（释义应为找不到）"),
]

# ---------- 生成输入 ----------

GEN_SYSTEM = """你在为一个桌面语音输入软件合成训练数据。软件的用法：用户按下快捷键说一段话，软件拿到语音识别（Qwen3-ASR）的原文，再结合当前应用、选中的文字、焦点是否为输入框来决定怎么处理。
你要写的是「这个人此刻真实会怎么说」，不是指令模板或书面语。
- spoken 是说话人实际说出的话。真实听写是边想边说的：句子常常很长、一逗到底；常带「就是」「然后」「那个」「嗯」「呃」「对」「其实」；偶尔重复半句、说到一半补一句、结尾不完整。改口（「周三，不对，周四」）只在设定要求时才写。
- 对软件下的指令往往很短（「英文」「短点」「翻一下」）。设定要求补充要求时，只能是结果里能体现的：语气、长度、保留某个词或写法、只处理其中某一部分、目标语言；不要写「抄送给谁」「发给谁」「把改动标出来」这类文字结果做不到的事。设定要求「先嘀咕一句背景」时，嘀咕的是这是发给谁的、为什么要改，不附带额外要求。
- asr_text 是 spoken 经 Qwen3-ASR 识别后的样子。这个识别器很准：常用词几乎不会错，不要制造「文章→蚊章」这类常用词错字。它的真实问题是：
  1. 口头语、叠字、重复、改口原样保留；
  2. 自带标点，大多是逗号，长停顿处用句号；每条最多一两处句号断在半句（如「然后。」），问句常缺问号；
  3. 英文缩写和产品名拆成字母（「O C R」「P R」）或识别成中文近音（「胡克」代 hook、「费尺」代 fetch、「瑞迪斯」代 Redis），中英混说时英文单词偶尔写成中文近音；
  4. 少见的专有名词、人名偶尔是同音字；数字有时写成汉字。
  干净=只有标点和口头语特征；轻度=再加 1 处 3、4 类问题；重度=断句很乱并有 2～3 处 3、4 类问题。
- screen 是当前窗口里能看到的文字（80～400 字，像截图文字识别的结果，可以零碎），要和应用、场景相符。
- 设定要求热词时，hotwords 给 2～6 个这个人常用的专有名词，其中至少一个出现在口述里且被识别错；否则为空数组。设定要求自我介绍时，context 写一句这个人的自我介绍（职业、在做什么）；否则为空字符串。
- 人名、公司名、产品名自己编，不要用真实个人信息；话题覆盖工作、生活、学习、技术、商务、家庭，各条之间不要雷同。
只输出 JSON 数组，每个元素 {"selection": 字符串或 null, "screen": 字符串, "hotwords": [字符串], "context": 字符串, "spoken": 字符串, "asr_text": 字符串}，顺序与给定设定一一对应。"""


def describe_frame(index, frame):
    parts = [f"场景={frame['scene']}", f"应用={frame['app']}", f"焦点{'可输入' if frame['editable'] else '不可输入'}", f"噪声={frame['noise']}", f"说法={frame['style']}"]
    for key, label in (("subtask", "子任务"), ("relation", "选中与口述的关系"), ("sel_lang", "选中文字语言"), ("sel_len", "选中文字长度"),
                       ("define_type", "词的类型"), ("say_lang", "口头要求的目标语言")):
        if frame.get(key):
            parts.append(f"{label}={frame[key]}")
    if frame["hotwords"]:
        parts.append("要热词")
    if frame["context"]:
        parts.append("要自我介绍")
    return f"{index + 1}. " + " ".join(parts)


def make_frames(rng, task, n):
    category = CATEGORIES[task["category"]]
    frames = []
    for i in range(n):
        scene = pick_scene(rng, category["scenes"])
        editable = category["editable"] if isinstance(category["editable"], bool) else rng.random() < category["editable"]
        frame = dict(scene=scene, app=rng.choice(SCENE_APPS[scene][0]), editable=editable, noise=pick(rng, NOISE),
                     hotwords=rng.random() < 0.2, context=rng.random() < 0.15, target_lang=pick(rng, TARGET_LANGS))
        frame["style"] = "整段都是内容" if task["category"].startswith("dictate") else pick(rng, SEL_STYLE if category["sel"] else SPEECH_TRANSLATE_STYLE)
        if task["category"] == "ask.no_input":
            frame["style"] = "直接提问"
        if task["category"] == "edit":
            frame["subtask"] = task["frame"].get("subtask") or (rng.choice(LOCAL_EDITS) if rng.random() < 0.6 else rng.choice(OTHER_EDITS))
        if task["category"] == "dictate.replace":
            frame["relation"] = category["relations"][i % 3]
        if category.get("sel_kind") == "text":
            frame["sel_lang"] = pick(rng, SEL_LANG)
            frame["sel_len"] = "100 字以内" if task["frame"].get("short") else pick(rng, [("一句", 30), ("一段（500 字内）", 50), ("长文（500～1500 字）", 20)])
        if task["category"] == "define":
            wanted = task["frame"].get("define_type")
            types = [t for t in DEFINE_TYPES if not t[0].startswith("生造")] if wanted == "常用" else DEFINE_TYPES
            frame["define_type"] = wanted if wanted and wanted != "常用" else pick(rng, types)
        # target_lang 是设置里的默认外语；say_lang 是用户口头点名的目标语言
        if task["category"] == "translate.speech" or (task["category"] == "translate.selection" and rng.random() < 0.6):
            frame["say_lang"] = pick(rng, TARGET_LANGS) if task["category"] == "translate.speech" or frame["sel_lang"] != "英文" else "中文"
        elif task["category"] == "translate.selection":
            frame["say_lang"] = "不说目标语言"
        if task["category"] not in ("translate.speech", "translate.selection", "define"):
            frame["target_lang"] = ""
        frames.append(frame)
    return frames


def generate_inputs(task, frames):
    category = CATEGORIES[task["category"]]
    user = (f"类别说明：{category['desc']}\n选中文字：{category['sel'] or '无（selection 为 null）'}\n\n逐条设定：\n"
            + "\n".join(describe_frame(i, f) for i, f in enumerate(frames))
            + f"\n\n生成 {len(frames)} 条，彼此的话题、说法、长度都要不同。")
    return parse_json(chat(GEN_SYSTEM, user, temperature=0.95))

# ---------- 对照组 ----------
# 每组里的变体按判定规则各有标签；shared 说明组内哪些部分相同

CONTRAST = [
    dict(name="同一请求句", shared="所有变体说同一句话（spoken 和 asr_text 完全相同），是一句可以对 AI 说、也可以对选中文字执行的请求，如「帮我总结一下」「列几个要点」「改得短一点」",
         variants=[(None, True, "AI 对话", "request"), ("text", True, "写作", "edit"), ("text", False, "浏览器", "edit")]),
    dict(name="同一问句", shared="所有变体说同一句问话（spoken 和 asr_text 完全相同），如「这个有什么问题」「这样写对吗」",
         variants=[(None, True, "AI 对话", "chat_question"), ("text", True, "代码编辑器", "question"), ("text", False, "浏览器", "question")]),
    dict(name="同一句翻译一下", shared="所有变体说同一句很短的翻译请求（spoken 和 asr_text 完全相同），如「翻译一下」「这个翻一下」",
         variants=[("text", False, "浏览器", "translate_sel"), ("word", False, "浏览器", "meaning"), ("word", True, "写作", "meaning")]),
    dict(name="同一选中三种说法", shared="所有变体的 selection、screen、应用完全相同（选中的是用户刚写的一段话）；第一条口述是重写的新内容，第二条是修改要求，第三条是对它的提问",
         variants=[("text", True, "聊天", "content"), ("text", True, "聊天", "edit"), ("text", True, "聊天", "question")]),
    dict(name="翻译口述与谈论翻译", shared="两条口述话题相同；第一条要求把口述内容翻成外语，第二条只是在聊天里谈到翻译这件事",
         variants=[(None, True, "聊天", "speech_translate"), (None, True, "聊天", "content")]),
    dict(name="同一个词两种问法", shared="两条的 selection（同一个中文或英文词）、screen、应用相同；第一条要求把它换成某种外语写法，第二条问它是什么意思",
         variants=[("word", True, "聊天", "lang_rewrite"), ("word", True, "聊天", "meaning")]),
    dict(name="同一问句有无输入框", shared="两条说同一句常识问题（spoken 和 asr_text 完全相同）；第一条焦点不是输入框，第二条是在聊天输入框里问朋友",
         variants=[(None, False, "通用", "question"), (None, True, "聊天", "chat_question")]),
]


def generate_contrast(rng, template):
    lines = []
    for i, (sel_kind, editable, scene, kind) in enumerate(template["variants"]):
        sel = {None: "无选中", "text": "选中一段文字", "word": "选中一个词"}[sel_kind]
        lines.append(f"{i + 1}. 场景={scene} 应用={rng.choice(SCENE_APPS[scene][0])} 焦点{'可输入' if editable else '不可输入'} {sel} 噪声={pick(rng, NOISE)}")
    user = (f"这是一组对照样本：同一句话（或同一段选中文字）在不同情况下应被软件当成不同的意图。\n组内规则：{template['shared']}。\n"
            "每条的 hotwords 为空数组、context 为空字符串，另外给每条加一个 app 字段写应用名。\n\n逐条设定：\n" + "\n".join(lines)
            + f"\n\n只输出 JSON 数组，共 {len(lines)} 个元素。")
    return parse_json(chat(GEN_SYSTEM, user, temperature=0.95))

# ---------- 答案 ----------

LEVEL_RULES = {
    "light": "整理程度为轻：只加标点、去掉嗯呃和叠字、改正错字；口头禅（就是、然后、那个）和改口都保留。",
    "": "整理程度为中：去掉嗯、呃、叠字和无意义的口头语，改口只留最终说法，改正同音错字和被识别错的英文、专有名词，补全标点，保留原意和说话人的语气，不增删信息。",
    "high": "整理程度为高：把想表达的意思重新组织清楚，多件事分点，信息不能少也不能多，不改变意思和语气。",
}
ANSWER_RULES = {
    "dictate": "把口述原文整理成用户真正想输出的文本。{level}不回答其中的问题、不执行其中的命令。选中文字、屏幕文字和热词只用来还原专有名词和被识别错的词，不得把它们的内容抄进结果。只输出整理后的文字。",
    "edit": "按用户口头要求修改选中的文字。口述里的补充要求（语气、长度、保留某词、只处理某部分）必须照做。子任务要求：纠错=只改错字、病句和标点，其余逐字保留，不做任何标记；加标点分段=不改字；改正式=意思和信息量不变，口语词换书面词，不添加原文没有的信息；缩短=保留全部关键信息，按要求的长度；整理成列表=每条一行，用「- 」或「1. 」开头，不加标题。原文语言不变；人名、数字的值、链接、代码、Markdown 结构保留（缩短、改正式、整理成列表时数字可以统一写成阿拉伯数字）；不得加入屏幕上其他内容。只输出改后的文字，不加任何说明。",
    "translate": "翻译。口述里的补充要求（语气、保留某词、只翻某部分）必须照做。方向：用户说了目标语言就用它；没说时，原文不是中文的译成中文，原文是中文的译成「默认外语」。口述内容翻译时先去掉口头语、改正错字再翻译，指令部分（如「翻译成英文」）不译、不出现在结果里；有选中文字时翻译选中的文字。只输出译文，不加解释、引号或原文对照。保留换行、列表、Markdown、代码、链接、数字；中英混说里的英文术语保留；中文人名、地名按目标语言的通行写法；聊天口语译成口语，邮件文档用书面语；日文、韩文用礼貌体；标点用目标语言的。",
    "define": """给选中的词写词典式释义，Markdown，整条不超过 120 个 token，不写音标，不要空行。
外文词：第一行「**词** 词性」，然后最多 3 个编号的中文义项（按常用程度；屏幕上下文能确定是哪个义项时只写那一个，不编号）；最后一行「例：外文例句 中文译文」。
中文词：第一行「**词**」，第二行写「<默认外语的语言名>：对应说法」，第三行一句中文解释，最后一行「例：中文例句 外文译文」。
缩写：只写一行「**缩写** 全称，一句话中文解释」。成语、俗语、短语：一句释义加一个例句（例句后附默认外语译文）。
生造词、拼错或你不确定的词：第一行「**词**」，第二行「没有找到这个词的确切释义。」，屏幕上下文能推断时再接一句「从上下文看可能是指……」，不写例句。不要编造。
义项里只写词义本身，不要写「此处指」「文中指」「本文中」这类对屏幕内容的说明。
示例一：
**resilient** 形容词
有韧性的；受挫后能迅速恢复的
例：The team stayed resilient after two delays. 两次延期后，团队依然保持了韧性。
示例二：
**内卷**
英文：involution；rat race（日常说法）
指同行之间过度竞争、投入增加但回报不增的现象。
例：这个行业太卷了。This industry is a total rat race.""",
}


def task_rules(sample):
    return ANSWER_RULES[sample["label"]].replace("{level}", LEVEL_RULES[sample["level"]])


def sample_view(sample):
    return (f"场景：{sample['scene']}（{sample['app']}）\n焦点：{'可输入' if sample['editable'] else '不可输入'}\n"
            f"默认外语：{sample['target_lang'] or '英文'}\n热词：{'、'.join(sample['hotwords']) or '（无）'}\n"
            f"屏幕文字：\n{sample['screen']}\n\n选中文字：{sample['selection'] or '（无）'}\n"
            + (f"子任务：{sample['subtask']}\n" if sample["subtask"] else "") + f"\n口述原文：{sample['text']}")


def generate_answer(sample):
    return chat("你是数据标注员，按规则写标准答案。\n" + task_rules(sample), sample_view(sample))

# ---------- 复核与质检 ----------

REVIEW_SYSTEM = """你是语音输入软件的意图判定员。按顺序判断，先命中的生效，只输出一个标签：dictate / edit / translate / define / ask。
1. 焦点不可输入：不可能是 dictate。有选中时按第 3 条，但「新内容」改判 ask；没有选中一律 ask。
2. 没有选中：明确要求把口述内容换成另一种语言的是 translate；其余一律 dictate（包括问句、祈使句、对其他 AI 或聊天对象的请求）。
3. 有选中：选中的是词级内容（单词、固定短语、缩写、成语，英文约 4 词内、中文约 6 字内、无句末标点）且想知道这个词本身的意思（翻译一下、什么意思、是什么、查一下）→ define；问的是它在当前内容里牵涉的事 → ask；选中词级内容并要求换成某种语言写法且焦点可输入 → translate；选中句段要求翻译 → translate；要求改、缩、扩、纠错、加标点、整理、续写、换语气、改成邮件、总结、加注释 → edit；对选中内容提问 → ask；其余（说的是用来替换选中的新内容）→ dictate。
4. 拿不准时 dictate（焦点不可输入时 ask）。"""

JUDGE_SYSTEM = "你是严格的数据质检员。判断待评答案是否完全符合任务规则、意思准确、语言地道、没有编造。屏幕文字只是参考：答案不需要处理屏幕上的其他内容。第一行只写「合格」或「不合格」，第二行起一句话说明主要问题。"


def review(sample):
    user = (f"场景：{sample['scene']}（{sample['app']}）\n焦点：{'可输入' if sample['editable'] else '不可输入'}\n"
            f"选中文字：{sample['selection'] or '（无）'}\n口述原文：{sample['text']}")
    match = re.search(r"dictate|edit|translate|define|ask", chat(REVIEW_SYSTEM, user))
    return match.group(0) if match else "?"


def judge(sample, effort):
    user = f"任务规则：\n{task_rules(sample)}\n\n{sample_view(sample)}\n\n待评答案：\n{sample['output']}"
    return chat(JUDGE_SYSTEM, user, effort=effort)


def cjk_ratio(text):
    letters = [c for c in text if c.isalpha()]
    return sum("一" <= c <= "鿿" for c in letters) / max(1, len(letters))


def hard_checks(sample):
    problems, out, label = [], sample["output"], sample["label"]
    if label in ("edit", "define") and not sample["selection"]:
        problems.append("缺选中")
    if label == "dictate" and not sample["editable"]:
        problems.append("不可输入却是听写")
    if not sample["text"].strip():
        problems.append("口述为空")
    if out and re.match(r"^(好的|以下是|修改如下|译文|翻译如下|整理后)", out):
        problems.append("答案带开场白")
    if label == "define" and out:
        if not out.startswith("**"):
            problems.append("释义格式")
        if len(out) > 260 or "\n\n" in out:
            problems.append("释义过长或有空行")
        if re.search(r"此处指|文中指|本文中", out):
            problems.append("释义带屏幕说明")
    if label == "translate" and out:
        source = sample["selection"] or sample["spoken"]
        if (cjk_ratio(source) > 0.5) == (cjk_ratio(out) > 0.5):
            problems.append("译文语言未变")
    return problems


def soft_checks(sample):
    warnings, sel = [], sample["selection"] or ""
    if sample["label"] == "dictate" and sel and sample["output"]:
        if any(sel[i:i + 10] in sample["output"] and sel[i:i + 10] not in sample["spoken"] for i in range(max(0, len(sel) - 9))):
            warnings.append("可能抄了选中文字")
    return warnings


def finish(sample):
    """生成答案、复核、质检，填好 keep 与原因。"""
    sample["review"] = review(sample)
    if sample["review"] != sample["label"]:
        sample.update(keep=False, reason=f"复核判为 {sample['review']}")
        return sample
    if needs_output(sample):
        sample["output"] = generate_answer(sample)
    problems = hard_checks(sample)
    if not problems and sample["output"]:
        for effort in ("low", "max"):
            verdict = judge(sample, effort)
            sample[f"judge_{effort}"] = verdict
            if verdict.startswith("不合格"):
                problems.append(f"质检 {effort}：" + verdict.split("\n", 1)[-1][:120])
                break
    sample["warnings"] = soft_checks(sample)
    sample.update(keep=not problems, reason="；".join(problems))
    return sample

# ---------- 组装样本 ----------

def base_sample(task, frame, item, kind, sel_kind, suffix):
    level = ""
    if CATEGORIES.get(task["category"], {}).get("kind") in ("content", "request", "chat_question") or kind in ("content", "request", "chat_question"):
        level = "light" if frame["scene"] == "聊天" else ""
        if random.random() < 0.1:
            level = random.choice(["light", "", "high"])
    selection = item.get("selection") if sel_kind else None
    return dict(id=f"{task['name']}-{suffix}", batch=task["batch"], split=task["split"], group=f"{task['name']}-{suffix}",
                origin="synthetic", category=task["category"], scene=frame["scene"], app=item.get("app") or frame["app"], level=level,
                editable=frame["editable"], selection=selection or None, hotwords=item.get("hotwords") or [], context=item.get("context") or "",
                screen=item.get("screen") or "", target_lang=frame.get("target_lang") or "", spoken=item.get("spoken") or "",
                text=item.get("asr_text") or "", noise=frame["noise"], subtask=frame.get("subtask", ""),
                label=derive_label(frame["editable"], sel_kind if selection else None, kind), output="")


def run_chunk(task, seed):
    rng = random.Random(seed)
    if task["category"] == "contrast":
        template = CONTRAST[seed % len(CONTRAST)]
        items = generate_contrast(rng, template)
        group = f"{task['name']}-{seed:08x}"
        samples = []
        for i, ((sel_kind, editable, scene, kind), item) in enumerate(zip(template["variants"], items)):
            frame = dict(scene=scene, app=item.get("app") or rng.choice(SCENE_APPS[scene][0]), editable=editable, noise="", target_lang="英文")
            sample = base_sample(task, frame, item, kind, sel_kind, f"{seed:08x}-{i}")
            sample.update(group=group, subtask="", contrast=template["name"])
            samples.append(sample)
        if len(samples) != len(template["variants"]):
            return []
        with ThreadPoolExecutor(len(samples)) as pool:
            samples = list(pool.map(finish, samples))
        if not all(s["keep"] for s in samples):  # 对照组要完整，缺一条整组丢弃
            for s in samples:
                s["keep"] = False
                s["reason"] = s["reason"] or "同组有样本被丢弃"
        return samples
    category = CATEGORIES[task["category"]]
    frames = make_frames(rng, task, PER_CALL)
    items = generate_inputs(task, frames)
    samples = [base_sample(task, f, it, category["kind"], category.get("sel_kind"), f"{seed:08x}-{i}") for i, (f, it) in enumerate(zip(frames, items))]
    with ThreadPoolExecutor(len(samples) or 1) as pool:
        return list(pool.map(finish, samples))

# ---------- 运行 ----------

def raw_path(task):
    return OUT / "raw" / f"{task['name']}.jsonl"


def load(task):
    path = raw_path(task)
    return [json.loads(line) for line in path.open()] if path.exists() else []


def kept_units(task, rows):
    if task["category"] == "contrast":
        return len({r["group"] for r in rows if r["keep"]})
    return sum(r["keep"] for r in rows)


def run_task(task):
    rows = load(task)
    lock = threading.Lock()
    path = raw_path(task)
    path.parent.mkdir(parents=True, exist_ok=True)
    attempts = len(rows)
    while kept_units(task, rows) < task["count"]:
        missing = task["count"] - kept_units(task, rows)
        per_chunk = 1 if task["category"] == "contrast" else PER_CALL * 0.8
        chunks = max(1, min(CONC * 2, round(missing / per_chunk + 0.5)))
        if attempts > task["count"] * (12 if task["category"] == "contrast" else 4):
            print(f"  {task['name']}：丢弃过多，停止（保留 {kept_units(task, rows)}/{task['count']}）", flush=True)
            return
        def one(n):
            seed = hash((task["name"], len(rows), n, time.time())) & 0xFFFFFFFF
            try:
                result = run_chunk(task, seed)
            except Exception as error:
                print(f"  {task['name']}：一批失败 {error}", flush=True)
                return []
            with lock, path.open("a") as f:
                for r in result:
                    f.write(json.dumps(r, ensure_ascii=False) + "\n")
                rows.extend(result)
            return result
        with ThreadPoolExecutor(CONC) as pool:
            done = sum(len(r) for r in pool.map(one, range(chunks)))
        attempts += done
        print(f"  {task['name']}：保留 {kept_units(task, rows)}/{task['count']}，累计生成 {len(rows)}", flush=True)


def stats():
    for task in JOBS:
        rows = load(task)
        if rows:
            reasons = {}
            for r in rows:
                if not r["keep"]:
                    key = r["reason"].split("：")[0].split("；")[0][:12]
                    reasons[key] = reasons.get(key, 0) + 1
            top = "，".join(f"{k} {v}" for k, v in sorted(reasons.items(), key=lambda x: -x[1])[:4])
            print(f"{task['name']}: 保留 {kept_units(task, rows)}/{task['count']}，生成 {len(rows)}；{top}")


FIELDS = ["id", "batch", "split", "group", "origin", "category", "scene", "app", "level", "editable", "selection", "hotwords", "context",
          "screen", "target_lang", "text", "label", "output", "subtask", "contrast", "spoken", "noise", "warnings"]


def pack():
    final = OUT / "final"
    final.mkdir(parents=True, exist_ok=True)
    files = {}
    for task in JOBS:
        for r in load(task):
            if r["keep"]:
                key = f"batch{task['batch']}-{task['split']}.jsonl"
                files.setdefault(key, []).append({k: r.get(k, "") for k in FIELDS})
    for name, rows in files.items():
        with (final / name).open("w") as f:
            for r in rows:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
        print(f"{name}: {len(rows)} 条")
    archive = OUT / f"intent-data-{time.strftime('%Y%m%d')}.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(final, arcname="intent-data")
        tar.add(ROOT / "docs/task/本地模型多任务微调/01-训练数据规范.md", arcname="intent-data/01-训练数据规范.md")
    print(archive)


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else "stats"
    if command == "run":
        prefixes = sys.argv[2:]
        for task in JOBS:
            if not prefixes or any(task["name"].startswith(p) for p in prefixes):
                print(f"{task['name']}（目标 {task['count']}）", flush=True)
                run_task(task)
    elif command == "stats":
        stats()
    elif command == "pack":
        pack()
