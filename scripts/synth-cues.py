"""生成内置提示音 ui/public/cues/<音色>.wav（44.1kHz 16bit 单声道）。

前端网页回退播放和后端原生播放都用这批文件；改音色后重新运行：python scripts/synth-cues.py ui/public/cues
「beep-*」与旧版 Web Audio 合成参数一致（峰值 0.25），新音色归一到同一峰值（-12 dBFS）。依赖 numpy。
"""
import sys
import wave
from pathlib import Path
import numpy as np

SR = 44100
OUT = Path(sys.argv[1])


def t_of(dur):
    return np.arange(int(SR * dur)) / SR


def exp_env(dur, attack=0.012, peak=1.0):
    """与现有 Web Audio 实现相同：指数上升到峰值，再指数衰减到 -80dB。"""
    t = t_of(dur)
    a = np.where(t < attack, 1e-4 * (peak / 1e-4) ** (t / attack), peak * (1e-4 / peak) ** ((t - attack) / (dur - attack)))
    return a


def place(chunks):
    """chunks: [(起始秒, 信号)] 混合到一条轨上。"""
    end = max(s + len(x) / SR for s, x in chunks) + 0.03
    out = np.zeros(int(end * SR))
    for s, x in chunks:
        i = int(s * SR)
        out[i:i + len(x)] += x
    return out


def legacy(freqs, dur, gap, shift=0):
    k = 2 ** (shift / 12)
    chunks, s = [], 0.01
    for f in freqs:
        chunks.append((s, 0.25 * exp_env(dur) * np.sin(2 * np.pi * f * k * t_of(dur))))
        s += dur + gap
    return place(chunks)


def drop(f0, f1, dur=0.11):
    """水滴：正弦快速滑音，带一点二次谐波。"""
    t = t_of(dur)
    f = f0 * (f1 / f0) ** np.minimum(t / 0.05, 1)
    ph = 2 * np.pi * np.cumsum(f) / SR
    env = np.minimum(t / 0.004, 1) * np.exp(-t / 0.035)
    return env * (np.sin(ph) + 0.12 * np.sin(2 * ph))


def marimba(f, dur=0.35):
    t = t_of(dur)
    parts = [(1.0, 1.0, 0.18), (3.93, 0.28, 0.05), (9.2, 0.08, 0.018)]
    x = sum(a * np.exp(-t / d) * np.sin(2 * np.pi * f * r * t) for r, a, d in parts)
    return np.minimum(t / 0.002, 1) * x


def bell(f, dur=0.6, idx=1.6, ratio=3.5):
    t = t_of(dur)
    mod = idx * np.exp(-t / 0.08) * np.sin(2 * np.pi * f * ratio * t)
    return np.minimum(t / 0.003, 1) * np.exp(-t / 0.22) * np.sin(2 * np.pi * f * t + mod)


def tick(f, dur=0.08):
    t = t_of(dur)
    rng = np.random.default_rng(7)
    noise = rng.standard_normal(len(t)) * np.exp(-t / 0.004) * 0.25
    body = np.exp(-t / 0.022) * np.sin(2 * np.pi * f * t) + 0.4 * np.exp(-t / 0.012) * np.sin(2 * np.pi * f * 2.76 * t)
    return np.minimum(t / 0.001, 1) * (body + noise)


def soft(f, dur=0.16):
    """柔和单音：正弦 + 轻微八度，缓起缓收。"""
    t = t_of(dur)
    env = np.sin(np.pi * np.minimum(t / 0.02, 0.5)) ** 2 * np.exp(-t / 0.06)
    return env * (np.sin(2 * np.pi * f * t) + 0.18 * np.sin(4 * np.pi * f * t))


def norm(x, peak_db=-6.0):
    return x / np.max(np.abs(x)) * 10 ** (peak_db / 20)


def save(name, x):
    data = (np.clip(x, -1, 1) * 32767).astype("<i2")
    with wave.open(str(OUT / f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(data.tobytes())


C5, D5, E5, G5, A5, C6 = 523.25, 587.33, 659.25, 783.99, 880.0, 1046.5
G4, A4, E4 = 392.0, 440.0, 329.63

SOUNDS = {
    "beep-up": legacy([660, 990], 0.1, 0.02),
    "beep-down": legacy([880, 520], 0.12, 0.02),
    "beep-double": legacy([880, 880], 0.07, 0.05),
    "drop-up": norm(drop(E5, A5), -12.0),
    "drop-down": norm(drop(A5, E5), -12.0),
    "marimba-up": norm(place([(0, marimba(G4)), (0.085, marimba(C5))]), -12.0),
    "marimba-down": norm(place([(0, marimba(C5)), (0.085, marimba(G4))]), -12.0),
    "bell-high": norm(bell(A5), -12.0),
    "bell-low": norm(bell(E5), -12.0),
    "tick-up": norm(place([(0, tick(1250)), (0.07, tick(1650))]), -12.0),
    "tick-down": norm(place([(0, tick(1650)), (0.07, tick(1250))]), -12.0),
    "soft-high": norm(soft(G5), -12.0),
    "soft-low": norm(soft(D5), -12.0),
    "arp-up": norm(place([(0, soft(C5, 0.12)), (0.055, soft(E5, 0.12)), (0.11, soft(G5, 0.18))]), -12.0),
    "arp-down": norm(place([(0, soft(G5, 0.12)), (0.055, soft(E5, 0.12)), (0.11, soft(C5, 0.18))]), -12.0),
    "marimba-low-up": norm(place([(0, marimba(E4)), (0.08, marimba(A4))]), -12.0),
    "marimba-low-down": norm(place([(0, marimba(A4)), (0.08, marimba(E4))]), -12.0),
}

if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for name, x in SOUNDS.items():
        save(name, x)
