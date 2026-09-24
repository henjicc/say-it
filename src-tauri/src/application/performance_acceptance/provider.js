// 验收插件无网络权限：只通过宿主的受限媒体句柄读取本地素材。
export default host => {
  let config = {};
  let received = 0;
  return {
    initialize(request) { config = request.config; },
    async invoke(request) {
      if (request.operation !== 'transcribeFile') throw new Error('unexpected operation');
      const runtime = globalThis.__sayitCreateRuntimeContext();
      const ref = request.payload.input.id;
      const description = await runtime.media.describe(ref);
      const header = await runtime.media.readChunk(ref, 0, 44);
      if (header.length !== 44 || String.fromCharCode(...header.slice(0, 4)) !== 'RIFF') {
        throw new Error('invalid fixture WAV');
      }
      host.storage.set('ready', config.nonce);
      if (config.mode === 'cancel') await new Promise(() => {});
      if (config.mode === 'failure') throw new Error('acceptance-provider-failure');
      let count = header.length;
      while (count < description.size) {
        const bytes = await runtime.media.readChunk(ref, count, Math.min(65536, description.size - count));
        if (!bytes.length) throw new Error('incomplete audio');
        count += bytes.length;
      }
      // 固定等待只用于采样运行态，不代表供应商识别耗时。
      await new Promise(resolve => setTimeout(resolve, 4000));
      const view = new DataView(new Uint8Array(header).buffer);
      const durationMs = Math.round((count - 44) * 1000 / view.getUint32(28, true));
      const sentences = Array.from({ length: 2000 }, (_, index) => ({
        beginTime: Math.floor(index * durationMs / 2000),
        endTime: Math.floor((index + 1) * durationMs / 2000),
        text: '本地验收语句。', words: [],
      }));
      return { durationMs, transcripts: [{ text: sentences.map(s => s.text).join(''), sentences }] };
    },
    realtimeStart() { received = 0; host.emit({ type: 'ready' }); },
    realtimeAudio(bytes) {
      received += bytes.length;
      if (config.mode === 'failure') throw new Error('acceptance-provider-failure');
      host.emit({ type: config.acceptanceScenario === 'subtitles' ? 'final' : 'partial', text: `实时验收:${received}` });
    },
    realtimeFinish() {
      if (config.mode === 'cancel') return new Promise(() => {});
      host.emit({ type: 'final', text: `实时验收:${received}` });
      host.emit({ type: 'finished' });
      return { ack: true };
    },
    realtimeStop() {},
  };
};
