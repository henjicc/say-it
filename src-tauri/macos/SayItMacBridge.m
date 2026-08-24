#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <AudioToolbox/AudioToolbox.h>
#import <CoreMedia/CoreMedia.h>
#import <IOKit/IOKitLib.h>
#import <IOKit/hid/IOHIDKeys.h>
#import <IOKit/hid/IOHIDManager.h>
#import <IOKit/hid/IOHIDUsageTables.h>
#import <IOKit/hidsystem/IOHIDLib.h>
#import <IOKit/hidsystem/IOHIDParameter.h>
#import <IOKit/hidsystem/IOHIDShared.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Vision/Vision.h>
#import <dlfcn.h>
#import <math.h>
#import <objc/runtime.h>

@interface SayItNonactivatingFloatingPanel : NSPanel
@end

@implementation SayItNonactivatingFloatingPanel
- (BOOL)canBecomeKeyWindow { return NO; }
- (BOOL)canBecomeMainWindow { return NO; }
@end

typedef bool (*SayItCapsLockCallback)(void *context, uint64_t flags);
typedef bool (*SayItFnKeyCallback)(void *context, bool pressed, uint64_t flags);
typedef bool (*SayItEscapeCallback)(void *context, bool pressed);
typedef void (*SayItAudioCallback)(void *context, const float *samples, size_t count);
typedef void (*SayItAudioErrorCallback)(void *context, const char *message);
typedef void (*SayItMouseMonitorCallback)(
    void *context,
    double x,
    double y,
    bool buttonDown,
    bool leftPressed,
    bool leftReleased
);

typedef struct {
    uint8_t *data;
    size_t length;
    uint32_t width;
    uint32_t height;
} SayItByteBuffer;

static char *SayItCopyString(NSString *value) {
    if (value == nil) return NULL;
    return strdup(value.UTF8String ?: "");
}

static void SayItSetError(char **error, NSString *message) {
    if (error != NULL) *error = SayItCopyString(message ?: @"未知 macOS 原生错误");
}

static void SayItRunOnMainThread(dispatch_block_t block);

static char *SayItCopyJSON(id value, char **error) {
    NSError *jsonError = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:&jsonError];
    if (data == nil) {
        SayItSetError(error, [NSString stringWithFormat:@"编码原生结果失败：%@", jsonError.localizedDescription]);
        return NULL;
    }
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    return SayItCopyString(json);
}

void sayit_macos_free_string(char *value) {
    free(value);
}

void sayit_macos_free_bytes(uint8_t *value) {
    free(value);
}

bool sayit_macos_decode_audio_file(const char *path, float **samples, size_t *count, char **error) {
    if (samples == NULL || count == NULL) {
        SayItSetError(error, @"macOS 音频解码输出参数无效");
        return false;
    }
    *samples = NULL;
    *count = 0;
    if (path == NULL) {
        SayItSetError(error, @"待解码音频路径无效");
        return false;
    }

    @autoreleasepool {
        NSString *filePath = [[NSString alloc] initWithUTF8String:path];
        if (filePath == nil) {
            SayItSetError(error, @"待解码音频路径不是有效的 UTF-8");
            return false;
        }
        ExtAudioFileRef audioFile = NULL;
        OSStatus status = ExtAudioFileOpenURL((__bridge CFURLRef)[NSURL fileURLWithPath:filePath], &audioFile);
        if (status != noErr || audioFile == NULL) {
            SayItSetError(error, [NSString stringWithFormat:@"打开 macOS 原生音频失败（OSStatus %d）", (int)status]);
            return false;
        }

        AudioStreamBasicDescription clientFormat = {0};
        clientFormat.mSampleRate = 16000.0;
        clientFormat.mFormatID = kAudioFormatLinearPCM;
        clientFormat.mFormatFlags = kAudioFormatFlagsNativeFloatPacked;
        clientFormat.mBytesPerPacket = sizeof(float);
        clientFormat.mFramesPerPacket = 1;
        clientFormat.mBytesPerFrame = sizeof(float);
        clientFormat.mChannelsPerFrame = 1;
        clientFormat.mBitsPerChannel = 8 * sizeof(float);
        status = ExtAudioFileSetProperty(
            audioFile,
            kExtAudioFileProperty_ClientDataFormat,
            sizeof(clientFormat),
            &clientFormat
        );
        if (status != noErr) {
            ExtAudioFileDispose(audioFile);
            SayItSetError(error, [NSString stringWithFormat:@"配置 macOS 音频转换失败（OSStatus %d）", (int)status]);
            return false;
        }

        NSMutableData *decoded = [NSMutableData data];
        float chunk[4096];
        while (true) {
            UInt32 frames = 4096;
            AudioBufferList buffers = {0};
            buffers.mNumberBuffers = 1;
            buffers.mBuffers[0].mNumberChannels = 1;
            buffers.mBuffers[0].mDataByteSize = sizeof(chunk);
            buffers.mBuffers[0].mData = chunk;
            status = ExtAudioFileRead(audioFile, &frames, &buffers);
            if (status != noErr) break;
            if (frames == 0) break;
            [decoded appendBytes:chunk length:(NSUInteger)frames * sizeof(float)];
        }
        ExtAudioFileDispose(audioFile);
        if (status != noErr) {
            SayItSetError(error, [NSString stringWithFormat:@"macOS 原生音频解码失败（OSStatus %d）", (int)status]);
            return false;
        }
        if (decoded.length == 0) {
            SayItSetError(error, @"macOS 原生音频解码没有输出音频数据");
            return false;
        }
        float *output = malloc(decoded.length);
        if (output == NULL) {
            SayItSetError(error, @"macOS 原生音频解码内存不足");
            return false;
        }
        memcpy(output, decoded.bytes, decoded.length);
        *samples = output;
        *count = decoded.length / sizeof(float);
        return true;
    }
}

static NSArray<NSDictionary<NSPasteboardType, NSData *> *> *SayItSnapshotPasteboard(NSPasteboard *pasteboard) {
    NSMutableArray<NSDictionary<NSPasteboardType, NSData *> *> *snapshot = [NSMutableArray array];
    for (NSPasteboardItem *item in pasteboard.pasteboardItems ?: @[]) {
        NSMutableDictionary<NSPasteboardType, NSData *> *types = [NSMutableDictionary dictionary];
        for (NSPasteboardType type in item.types) {
            NSData *data = [item dataForType:type];
            if (data != nil) types[type] = [data copy];
        }
        if (types.count > 0) [snapshot addObject:types];
    }
    return snapshot;
}

static bool SayItRestorePasteboard(
    NSPasteboard *pasteboard,
    NSArray<NSDictionary<NSPasteboardType, NSData *> *> *snapshot
) {
    NSMutableArray<NSPasteboardItem *> *items = [NSMutableArray arrayWithCapacity:snapshot.count];
    for (NSDictionary<NSPasteboardType, NSData *> *types in snapshot) {
        NSPasteboardItem *item = [[NSPasteboardItem alloc] init];
        for (NSPasteboardType type in types) {
            if (![item setData:types[type] forType:type]) return false;
        }
        [items addObject:item];
    }
    // selection-hook uses the host-only preparation path so restoring temporary
    // clipboard contents does not trigger Universal Clipboard / Handoff syncing.
    [pasteboard prepareForNewContentsWithOptions:NSPasteboardContentsCurrentHostOnly];
    return items.count == 0 || [pasteboard writeObjects:items];
}

static bool SayItPostPasteShortcut(char **error) {
    // 直接发送物理 Command+V，避免从后台听写线程调用只能在主队列使用的
    // Text Services Manager 键盘布局查询 API。
    const CGKeyCode commandKeyCode = 0x37;
    const CGKeyCode vKeyCode = 0x09;
    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    if (source == NULL) {
        SayItSetError(error, @"无法创建 macOS 键盘事件源");
        return false;
    }

    CGEventRef commandDown = CGEventCreateKeyboardEvent(source, commandKeyCode, true);
    CGEventRef vDown = CGEventCreateKeyboardEvent(source, vKeyCode, true);
    CGEventRef vUp = CGEventCreateKeyboardEvent(source, vKeyCode, false);
    CGEventRef commandUp = CGEventCreateKeyboardEvent(source, commandKeyCode, false);
    if (commandDown == NULL || vDown == NULL || vUp == NULL || commandUp == NULL) {
        if (commandDown != NULL) CFRelease(commandDown);
        if (vDown != NULL) CFRelease(vDown);
        if (vUp != NULL) CFRelease(vUp);
        if (commandUp != NULL) CFRelease(commandUp);
        CFRelease(source);
        SayItSetError(error, @"无法创建 macOS 粘贴键盘事件");
        return false;
    }

    CGEventSetFlags(commandDown, kCGEventFlagMaskCommand);
    CGEventSetFlags(vDown, kCGEventFlagMaskCommand);
    CGEventSetFlags(vUp, kCGEventFlagMaskCommand);
    CGEventSetFlags(commandUp, 0);
    CGEventPost(kCGHIDEventTap, commandDown);
    CGEventPost(kCGHIDEventTap, vDown);
    CGEventPost(kCGHIDEventTap, vUp);
    CGEventPost(kCGHIDEventTap, commandUp);

    CFRelease(commandDown);
    CFRelease(vDown);
    CFRelease(vUp);
    CFRelease(commandUp);
    CFRelease(source);
    return true;
}

bool sayit_macos_paste_current_clipboard(char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"模拟粘贴需要辅助功能权限");
        return false;
    }
    return SayItPostPasteShortcut(error);
}

bool sayit_macos_press_return(uint32_t processId, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"模拟回车需要辅助功能权限");
        return false;
    }
    if (processId == 0) {
        SayItSetError(error, @"模拟回车的目标应用无效");
        return false;
    }
    const CGKeyCode returnKeyCode = 0x24;
    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    CGEventRef keyDown = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, returnKeyCode, true);
    CGEventRef keyUp = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, returnKeyCode, false);
    if (source == NULL || keyDown == NULL || keyUp == NULL) {
        if (keyDown != NULL) CFRelease(keyDown);
        if (keyUp != NULL) CFRelease(keyUp);
        if (source != NULL) CFRelease(source);
        SayItSetError(error, @"无法创建 macOS 回车键事件");
        return false;
    }
    // 点击非激活悬浮球时目标应用仍持有键盘焦点；定向投递可以避免事件被
    // WebView 的鼠标处理阶段或其他前台状态变化截获。
    CGEventPostToPid((pid_t)processId, keyDown);
    CGEventPostToPid((pid_t)processId, keyUp);
    CFRelease(keyDown);
    CFRelease(keyUp);
    CFRelease(source);
    return true;
}

bool sayit_macos_paste_text(const char *text, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"模拟粘贴需要辅助功能权限");
        return false;
    }
    if (text == NULL) {
        SayItSetError(error, @"待粘贴文本无效");
        return false;
    }

    NSString *value = [[NSString alloc] initWithUTF8String:text];
    if (value == nil) {
        SayItSetError(error, @"待粘贴文本不是有效的 UTF-8");
        return false;
    }

    __block NSPasteboard *pasteboard = nil;
    __block NSArray<NSDictionary<NSPasteboardType, NSData *> *> *snapshot = nil;
    __block NSInteger injectedChangeCount = 0;
    __block bool clipboardReady = false;
    __block bool clipboardChangedDuringSnapshot = false;
    SayItRunOnMainThread(^{
        pasteboard = NSPasteboard.generalPasteboard;
        NSInteger sourceChangeCount = pasteboard.changeCount;
        snapshot = SayItSnapshotPasteboard(pasteboard);
        if (pasteboard.changeCount != sourceChangeCount) {
            clipboardChangedDuringSnapshot = true;
            return;
        }
        [pasteboard clearContents];
        clipboardReady = [pasteboard setString:value forType:NSPasteboardTypeString];
        injectedChangeCount = pasteboard.changeCount;
    });
    if (clipboardChangedDuringSnapshot) {
        SayItSetError(error, @"剪贴板在准备粘贴时已被更新，请重试");
        return false;
    }
    if (!clipboardReady) {
        SayItRunOnMainThread(^{
            if (pasteboard.changeCount == injectedChangeCount) {
                SayItRestorePasteboard(pasteboard, snapshot);
            }
        });
        SayItSetError(error, @"写入 macOS 剪贴板失败");
        return false;
    }

    if (!SayItPostPasteShortcut(error)) {
        SayItRunOnMainThread(^{
            if (pasteboard.changeCount == injectedChangeCount) {
                SayItRestorePasteboard(pasteboard, snapshot);
            }
        });
        return false;
    }

    // 等目标应用读取完剪贴板再恢复。若期间用户或其他应用主动修改了剪贴板，
    // changeCount 会变化，此时不能用旧快照覆盖对方的新内容。
    [NSThread sleepForTimeInterval:0.18];
    __block bool restored = true;
    SayItRunOnMainThread(^{
        if (pasteboard.changeCount == injectedChangeCount) {
            restored = SayItRestorePasteboard(pasteboard, snapshot);
        }
    });
    if (!restored) {
        SayItSetError(error, @"恢复 macOS 剪贴板失败");
        return false;
    }
    return true;
}

static bool SayItPostUnicodeChunk(CGEventSourceRef source, NSString *value, NSRange range) {
    UniChar *characters = malloc(range.length * sizeof(UniChar));
    if (characters == NULL) return false;
    [value getCharacters:characters range:range];

    CGEventRef keyDown = CGEventCreateKeyboardEvent(source, 0, true);
    CGEventRef keyUp = CGEventCreateKeyboardEvent(source, 0, false);
    if (keyDown == NULL || keyUp == NULL) {
        if (keyDown != NULL) CFRelease(keyDown);
        if (keyUp != NULL) CFRelease(keyUp);
        free(characters);
        return false;
    }
    CGEventSetFlags(keyDown, 0);
    CGEventSetFlags(keyUp, 0);
    CGEventKeyboardSetUnicodeString(keyDown, range.length, characters);
    CGEventPost(kCGHIDEventTap, keyDown);
    CGEventPost(kCGHIDEventTap, keyUp);
    CFRelease(keyDown);
    CFRelease(keyUp);
    free(characters);
    return true;
}

bool sayit_macos_type_text(const char *text, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"逐字输入需要辅助功能权限");
        return false;
    }
    if (text == NULL) {
        SayItSetError(error, @"待输入文本无效");
        return false;
    }
    NSString *value = [[NSString alloc] initWithUTF8String:text];
    if (value == nil) {
        SayItSetError(error, @"待输入文本不是有效的 UTF-8");
        return false;
    }
    if (value.length == 0) return true;

    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    if (source == NULL) {
        SayItSetError(error, @"无法创建 macOS 文字输入事件源");
        return false;
    }

    // Quartz 使用 UTF-16；按组合字符边界分批，既避免拆开 emoji/代理对，也避免部分
    // 应用丢弃单个事件里过长的 Unicode 文本。
    const NSUInteger maxChunkLength = 32;
    NSUInteger cursor = 0;
    NSRange chunk = NSMakeRange(0, 0);
    bool success = true;
    while (cursor < value.length) {
        NSRange character = [value rangeOfComposedCharacterSequenceAtIndex:cursor];
        if (chunk.length > 0 && NSMaxRange(character) - chunk.location > maxChunkLength) {
            if (!SayItPostUnicodeChunk(source, value, chunk)) {
                success = false;
                break;
            }
            chunk = NSMakeRange(character.location, 0);
        }
        if (chunk.length == 0) chunk.location = character.location;
        chunk.length = NSMaxRange(character) - chunk.location;
        cursor = NSMaxRange(character);
    }
    if (success && chunk.length > 0) success = SayItPostUnicodeChunk(source, value, chunk);
    CFRelease(source);

    if (!success) {
        SayItSetError(error, @"创建 macOS Unicode 输入事件失败");
        return false;
    }
    return true;
}

static void SayItRunOnMainThread(dispatch_block_t block) {
    if (NSThread.isMainThread) {
        block();
    } else {
        dispatch_sync(dispatch_get_main_queue(), block);
    }
}

enum {
    SayItContextOcrAccessibilityPermission = 1u << 0,
    SayItContextOcrScreenCapturePermission = 1u << 1,
};

static bool SayItAccessibilityAccess(bool request) {
    __block bool trusted = false;
    SayItRunOnMainThread(^{
        trusted = AXIsProcessTrusted();
        if (request && !trusted) {
            NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
            (void)AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
            // 申请函数只负责触发系统设置提示；必须再次检查，不能把“已提示”当成“已授权”。
            trusted = AXIsProcessTrusted();
        }
    });
    return trusted;
}

static bool SayItScreenCaptureAccess(bool request) {
    __block bool granted = false;
    SayItRunOnMainThread(^{
        granted = CGPreflightScreenCaptureAccess();
        if (request && !granted) {
            (void)CGRequestScreenCaptureAccess();
            // 用户可能需要在系统设置中手动确认，申请返回后仍要重新预检。
            granted = CGPreflightScreenCaptureAccess();
        }
    });
    return granted;
}

static uint32_t SayItContextOcrPermissionBits(bool request) {
    uint32_t bits = 0;
    if (SayItAccessibilityAccess(request)) bits |= SayItContextOcrAccessibilityPermission;
    if (SayItScreenCaptureAccess(request)) bits |= SayItContextOcrScreenCapturePermission;
    return bits;
}

uint32_t sayit_macos_context_ocr_permissions(bool request) {
    return SayItContextOcrPermissionBits(request);
}

bool sayit_macos_accessibility_permission(bool request) {
    return SayItAccessibilityAccess(request);
}

char *sayit_macos_copy_selection_text(uint32_t processId, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"复制当前选区需要辅助功能权限");
        return NULL;
    }
    NSRunningApplication *frontmost = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (processId == 0 || frontmost.processIdentifier != (pid_t)processId) {
        SayItSetError(error, @"当前前台应用与选区来源不一致");
        return NULL;
    }

    __block NSPasteboard *pasteboard = nil;
    __block NSArray<NSDictionary<NSPasteboardType, NSData *> *> *snapshot = nil;
    __block NSInteger temporaryChangeCount = 0;
    SayItRunOnMainThread(^{
        pasteboard = NSPasteboard.generalPasteboard;
        NSInteger sourceChangeCount = pasteboard.changeCount;
        snapshot = SayItSnapshotPasteboard(pasteboard);
        if (pasteboard.changeCount == sourceChangeCount) {
            [pasteboard clearContents];
            temporaryChangeCount = pasteboard.changeCount;
        }
    });
    if (pasteboard == nil || snapshot == nil || temporaryChangeCount == 0) {
        SayItSetError(error, @"备份 macOS 剪贴板失败");
        return NULL;
    }

    CGEventSourceRef source = CGEventSourceCreate(kCGEventSourceStateCombinedSessionState);
    const CGKeyCode commandKeyCode = 0x37;
    const CGKeyCode cKeyCode = 0x08;
    CGEventRef commandDown = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, commandKeyCode, true);
    CGEventRef cDown = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, cKeyCode, true);
    CGEventRef cUp = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, cKeyCode, false);
    CGEventRef commandUp = source == NULL ? NULL : CGEventCreateKeyboardEvent(source, commandKeyCode, false);
    if (source == NULL || commandDown == NULL || cDown == NULL || cUp == NULL || commandUp == NULL) {
        if (commandDown != NULL) CFRelease(commandDown);
        if (cDown != NULL) CFRelease(cDown);
        if (cUp != NULL) CFRelease(cUp);
        if (commandUp != NULL) CFRelease(commandUp);
        if (source != NULL) CFRelease(source);
        SayItRunOnMainThread(^{
            if (pasteboard.changeCount == temporaryChangeCount) SayItRestorePasteboard(pasteboard, snapshot);
        });
        SayItSetError(error, @"无法创建 macOS 复制选区事件");
        return NULL;
    }
    CGEventSetFlags(commandDown, kCGEventFlagMaskCommand);
    CGEventSetFlags(cDown, kCGEventFlagMaskCommand);
    CGEventSetFlags(cUp, kCGEventFlagMaskCommand);
    CGEventPost(kCGHIDEventTap, commandDown);
    CGEventPost(kCGHIDEventTap, cDown);
    CGEventPost(kCGHIDEventTap, cUp);
    CGEventPost(kCGHIDEventTap, commandUp);
    CFRelease(commandDown);
    CFRelease(cDown);
    CFRelease(cUp);
    CFRelease(commandUp);
    CFRelease(source);

    [NSThread sleepForTimeInterval:0.12];
    __block NSString *selected = nil;
    __block NSInteger copiedChangeCount = 0;
    __block bool restored = true;
    __block bool clipboardWasChangedByUser = false;
    SayItRunOnMainThread(^{
        copiedChangeCount = pasteboard.changeCount;
        if (copiedChangeCount > temporaryChangeCount) {
            selected = [pasteboard stringForType:NSPasteboardTypeString];
        }
    });
    [NSThread sleepForTimeInterval:0.04];
    SayItRunOnMainThread(^{
        if (pasteboard.changeCount == copiedChangeCount) {
            restored = SayItRestorePasteboard(pasteboard, snapshot);
        } else {
            clipboardWasChangedByUser = true;
        }
    });
    NSRunningApplication *currentFrontmost = NSWorkspace.sharedWorkspace.frontmostApplication;
    if (clipboardWasChangedByUser) {
        SayItSetError(error, @"复制选区期间剪贴板被用户修改，已保留用户的新内容并放弃本次结果");
        return NULL;
    }
    if (!restored) {
        SayItSetError(error, @"恢复 macOS 剪贴板失败");
        return NULL;
    }
    if (currentFrontmost.processIdentifier != (pid_t)processId) {
        SayItSetError(error, @"复制选区期间前台应用已变化，已放弃本次结果");
        return NULL;
    }
    if (selected.length == 0) {
        SayItSetError(error, @"当前应用没有可复制的文本选区");
        return NULL;
    }
    return SayItCopyString(selected);
}

char *sayit_macos_system_fonts_json(char **error) {
    __block NSArray<NSString *> *families = nil;
    SayItRunOnMainThread(^{
        families = [NSFontManager.sharedFontManager.availableFontFamilies
            sortedArrayUsingSelector:@selector(localizedCaseInsensitiveCompare:)];
    });
    if (families == nil) {
        SayItSetError(error, @"macOS 没有返回可用字体列表");
        return NULL;
    }
    return SayItCopyJSON(families, error);
}

bool sayit_macos_volume_available_capacity(
    const char *path,
    uint64_t *capacity,
    char **error
) {
    if (path == NULL || capacity == NULL) {
        SayItSetError(error, @"macOS 磁盘容量查询参数无效");
        return false;
    }
    NSURL *url = [NSURL fileURLWithFileSystemRepresentation:path isDirectory:YES relativeToURL:nil];
    if (url == nil) {
        SayItSetError(error, @"无法解析 macOS 数据目录路径");
        return false;
    }

    NSError *capacityError = nil;
    NSNumber *available = nil;
    if (![url getResourceValue:&available
                        forKey:NSURLVolumeAvailableCapacityForImportantUsageKey
                         error:&capacityError] || available == nil) {
        capacityError = nil;
        if (![url getResourceValue:&available
                            forKey:NSURLVolumeAvailableCapacityKey
                             error:&capacityError] || available == nil) {
            SayItSetError(error, [NSString stringWithFormat:@"读取 macOS 磁盘剩余空间失败：%@",
                capacityError.localizedDescription ?: @"未知错误"]);
            return false;
        }
    }
    long long value = available.longLongValue;
    if (value < 0) {
        SayItSetError(error, @"macOS 返回了无效的磁盘剩余空间");
        return false;
    }
    *capacity = (uint64_t)value;
    return true;
}

static NSScreen *SayItIndicatorScreen(NSWindow *window) {
    // mainScreen 对应当前拥有键盘焦点窗口所在屏幕；全局快捷键触发时就是用户正在听写的应用。
    // 回退到悬浮窗当前屏幕，最后才使用主显示器。
    return NSScreen.mainScreen ?: window.screen ?: NSScreen.screens.firstObject;
}

bool sayit_macos_place_indicator_window(
    void *nsWindow,
    double width,
    double height,
    int32_t anchor,
    double offsetY,
    char **error
) {
    if (nsWindow == NULL || width <= 0 || height <= 0) {
        SayItSetError(error, @"macOS 悬浮窗口定位参数无效");
        return false;
    }
    __block bool success = false;
    SayItRunOnMainThread(^{
        NSWindow *window = (__bridge NSWindow *)nsWindow;
        NSScreen *screen = SayItIndicatorScreen(window);
        if (screen == nil) return;
        NSRect frame = screen.frame;
        NSRect visible = screen.visibleFrame;
        CGFloat x;
        CGFloat y;
        if (anchor == 0) {
            x = NSMidX(visible) - width / 2.0;
            y = NSMaxY(visible) - height - offsetY;
        } else if (anchor == 1) {
            x = NSMidX(visible) - width / 2.0;
            // 正 offset 与其他平台保持一致：视觉上向屏幕下方移动。
            y = NSMidY(visible) - height / 2.0 - offsetY;
        } else {
            // 底部 Dock 会抬高 visibleFrame 的下边缘，按该边缘自动适配 Dock 大小。
            // Dock 在左/右侧时，下边缘没有被占用，因此水平居中和底部锚点都按完整屏幕计算。
            BOOL hasBottomInset = NSMinY(visible) > NSMinY(frame) + 0.5;
            CGFloat bottomEdge = hasBottomInset ? NSMinY(visible) : NSMinY(frame);
            x = NSMidX(frame) - width / 2.0;
            y = bottomEdge + offsetY;
        }
        [window setFrame:NSMakeRect(x, y, width, height) display:NO];
        success = true;
    });
    if (!success) SayItSetError(error, @"macOS 没有可用的显示器");
    return success;
}

bool sayit_macos_indicator_visible_screen_size(
    void *nsWindow,
    double *width,
    double *height,
    char **error
) {
    if (nsWindow == NULL || width == NULL || height == NULL) {
        SayItSetError(error, @"macOS 可用屏幕区域参数无效");
        return false;
    }
    __block bool success = false;
    __block NSSize size = NSZeroSize;
    SayItRunOnMainThread(^{
        NSWindow *window = (__bridge NSWindow *)nsWindow;
        NSScreen *screen = SayItIndicatorScreen(window);
        if (screen == nil) return;
        size = screen.visibleFrame.size;
        success = true;
    });
    if (!success) {
        SayItSetError(error, @"macOS 没有可用的显示器");
        return false;
    }
    *width = size.width;
    *height = size.height;
    return true;
}

bool sayit_macos_configure_floating_orb_window(void *nsWindow, bool nonactivating, char **error) {
    if (nsWindow == NULL) {
        SayItSetError(error, @"macOS 悬浮球窗口参数无效");
        return false;
    }
    SayItRunOnMainThread(^{
        NSWindow *window = (__bridge NSWindow *)nsWindow;
        if (nonactivating && object_getClass(window) != SayItNonactivatingFloatingPanel.class) {
            object_setClass(window, SayItNonactivatingFloatingPanel.class);
            window.styleMask = NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel;
            NSPanel *panel = (NSPanel *)window;
            panel.floatingPanel = YES;
            panel.becomesKeyOnlyIfNeeded = YES;
        }
        window.collectionBehavior = window.collectionBehavior
            | NSWindowCollectionBehaviorCanJoinAllSpaces
            | NSWindowCollectionBehaviorFullScreenAuxiliary;
        window.hidesOnDeactivate = NO;
    });
    return true;
}

bool sayit_macos_floating_orb_owns_pointer_event(void *nsWindow) {
    if (nsWindow == NULL) return false;
    __block bool ownsPointerEvent = false;
    SayItRunOnMainThread(^{
        NSWindow *window = (__bridge NSWindow *)nsWindow;
        NSEvent *event = NSApp.currentEvent;
        bool eventBelongsToWindow = event != nil && event.window == window;
        bool cursorInsideWindow = NSPointInRect(NSEvent.mouseLocation, window.frame);
        ownsPointerEvent = eventBelongsToWindow || cursorInsideWindow;
    });
    return ownsPointerEvent;
}

static NSDictionary *SayItWindowInfoForApplication(NSRunningApplication *application, CGWindowID targetWindowId) {
    if (application == nil || application.processIdentifier <= 0) return nil;
    CFArrayRef rawWindows = CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID
    );
    NSArray *windows = CFBridgingRelease(rawWindows);
    NSDictionary *selected = nil;
    for (NSDictionary *window in windows) {
        NSNumber *ownerPid = window[(id)kCGWindowOwnerPID];
        NSNumber *layer = window[(id)kCGWindowLayer];
        NSNumber *alpha = window[(id)kCGWindowAlpha];
        NSNumber *windowId = window[(id)kCGWindowNumber];
        NSDictionary *bounds = window[(id)kCGWindowBounds];
        if (ownerPid.intValue != application.processIdentifier || layer.intValue != 0 || alpha.doubleValue <= 0) continue;
        if (targetWindowId != 0 && windowId.unsignedIntValue != targetWindowId) continue;
        if ([bounds[@"Width"] doubleValue] < 2 || [bounds[@"Height"] doubleValue] < 2) continue;
        selected = window;
        break;
    }
    if (selected == nil) return nil;
    NSString *processName = application.executableURL.lastPathComponent ?: application.localizedName ?: @"";
    NSString *appName = application.localizedName ?: processName.stringByDeletingPathExtension;
    NSString *title = selected[(id)kCGWindowName];
    return @{
        @"windowId": selected[(id)kCGWindowNumber] ?: @0,
        @"processId": @(application.processIdentifier),
        @"processName": processName,
        @"appName": appName,
        @"windowTitle": title ?: [NSNull null],
    };
}

char *sayit_macos_frontmost_window_json(char **error) {
    @autoreleasepool {
        NSRunningApplication *application = NSWorkspace.sharedWorkspace.frontmostApplication;
        if (application == nil || application.processIdentifier == NSProcessInfo.processInfo.processIdentifier) {
            SayItSetError(error, @"未找到其他前台应用");
            return NULL;
        }
        NSDictionary *window = SayItWindowInfoForApplication(application, 0);
        if (window == nil) {
            SayItSetError(error, @"前台应用没有可捕获的屏幕窗口");
            return NULL;
        }
        return SayItCopyJSON(window, error);
    }
}

bool sayit_macos_activate_application(uint32_t processId, char **error) {
    @autoreleasepool {
        NSRunningApplication *application =
            [NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)processId];
        if (application == nil || application.terminated) {
            SayItSetError(error, @"听写开始时的目标应用已经退出");
            return false;
        }
        if (![application activateWithOptions:0]) {
            SayItSetError(error, @"无法重新激活听写开始时的目标应用");
            return false;
        }
        return true;
    }
}

char *sayit_macos_window_json(uint32_t windowId, uint32_t processId, char **error) {
    @autoreleasepool {
        NSRunningApplication *application = [NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)processId];
        NSDictionary *window = SayItWindowInfoForApplication(application, (CGWindowID)windowId);
        if (window == nil) {
            SayItSetError(error, @"目标 macOS 窗口已经关闭或不可见");
            return NULL;
        }
        return SayItCopyJSON(window, error);
    }
}

char *sayit_macos_running_apps_json(char **error) {
    @autoreleasepool {
        NSMutableArray *items = [NSMutableArray array];
        pid_t ownPid = NSProcessInfo.processInfo.processIdentifier;
        for (NSRunningApplication *application in NSWorkspace.sharedWorkspace.runningApplications) {
            if (application.processIdentifier == ownPid || application.activationPolicy != NSApplicationActivationPolicyRegular) continue;
            NSDictionary *window = SayItWindowInfoForApplication(application, 0);
            if (window != nil) [items addObject:window];
        }
        return SayItCopyJSON(items, error);
    }
}

char *sayit_macos_application_bundle_json(const char *path, char **error) {
    @autoreleasepool {
        if (path == NULL) {
            SayItSetError(error, @"macOS 应用路径无效");
            return NULL;
        }
        NSString *bundlePath = [[NSFileManager defaultManager]
            stringWithFileSystemRepresentation:path
            length:strlen(path)];
        NSBundle *bundle = [NSBundle bundleWithPath:bundlePath];
        NSURL *executableURL = bundle.executableURL;
        if (bundle == nil || executableURL == nil) {
            SayItSetError(error, @"请选择有效的 macOS .app 应用包");
            return NULL;
        }
        NSString *processName = executableURL.lastPathComponent;
        NSString *appName = [bundle objectForInfoDictionaryKey:@"CFBundleDisplayName"]
            ?: [bundle objectForInfoDictionaryKey:@"CFBundleName"]
            ?: bundlePath.lastPathComponent.stringByDeletingPathExtension;
        if (processName.length == 0 || appName.length == 0) {
            SayItSetError(error, @"macOS 应用包缺少可执行文件或显示名称");
            return NULL;
        }
        return SayItCopyJSON(@{
            @"processName": processName,
            @"appName": appName,
        }, error);
    }
}

int32_t sayit_macos_focused_input_security(uint32_t processId, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"检查密码输入区域需要辅助功能权限");
        return -1;
    }
    AXUIElementRef application = AXUIElementCreateApplication((pid_t)processId);
    AXUIElementSetMessagingTimeout(application, 0.5f);
    CFTypeRef focused = NULL;
    AXError focusedError = AXUIElementCopyAttributeValue(application, kAXFocusedUIElementAttribute, &focused);
    CFRelease(application);
    if (focusedError != kAXErrorSuccess || focused == NULL) {
        SayItSetError(error, @"无法确认当前输入区域是否受保护，已停止 OCR 截图");
        return -1;
    }
    CFTypeRef role = NULL;
    AXError roleError = AXUIElementCopyAttributeValue((AXUIElementRef)focused, kAXSubroleAttribute, &role);
    CFRelease(focused);
    if (roleError == kAXErrorAttributeUnsupported || roleError == kAXErrorNoValue) return 0;
    if (roleError != kAXErrorSuccess || role == NULL) {
        SayItSetError(error, @"无法读取当前输入区域类型，已停止 OCR 截图");
        return -1;
    }
    bool secure = CFGetTypeID(role) == CFStringGetTypeID()
        && CFEqual(role, kAXSecureTextFieldSubrole);
    CFRelease(role);
    return secure ? 1 : 0;
}

static NSString *SayItAXStringAttribute(AXUIElementRef element, CFStringRef attribute) {
    CFTypeRef value = NULL;
    AXError result = AXUIElementCopyAttributeValue(element, attribute, &value);
    if (result != kAXErrorSuccess || value == NULL) return nil;
    NSString *text = nil;
    if (CFGetTypeID(value) == CFStringGetTypeID()) {
        text = [(__bridge NSString *)value copy];
    } else if (CFGetTypeID(value) == CFAttributedStringGetTypeID()) {
        CFStringRef string = CFAttributedStringGetString((CFAttributedStringRef)value);
        if (string != NULL) text = [(__bridge NSString *)string copy];
    }
    CFRelease(value);
    return text;
}

// selection-hook 会在 AXSelectedText 不可用时用 AXValue + AXSelectedTextRange
// 还原选区。部分原生控件、WebKit 控件只实现后一组属性，因此这里保留同样的回退。
static NSString *SayItAXSelectedTextFromElement(AXUIElementRef element) {
    CFTypeRef selectedValue = NULL;
    AXError selectedError = AXUIElementCopyAttributeValue(element, kAXSelectedTextAttribute, &selectedValue);
    if (selectedError == kAXErrorSuccess && selectedValue != NULL) {
        NSString *selected = nil;
        if (CFGetTypeID(selectedValue) == CFStringGetTypeID()) {
            selected = [(__bridge NSString *)selectedValue copy];
        } else if (CFGetTypeID(selectedValue) == CFAttributedStringGetTypeID()) {
            CFStringRef string = CFAttributedStringGetString((CFAttributedStringRef)selectedValue);
            if (string != NULL) selected = [(__bridge NSString *)string copy];
        } else if (CFGetTypeID(selectedValue) == CFNumberGetTypeID()) {
            selected = [(__bridge NSNumber *)selectedValue stringValue];
        }
        CFRelease(selectedValue);
        if (selected.length > 0) return selected;
    } else if (selectedValue != NULL) {
        CFRelease(selectedValue);
    }

    NSString *value = SayItAXStringAttribute(element, kAXValueAttribute);
    if (value.length == 0) return nil;
    CFTypeRef rangeValue = NULL;
    AXError rangeError = AXUIElementCopyAttributeValue(
        element,
        kAXSelectedTextRangeAttribute,
        &rangeValue
    );
    if (rangeError != kAXErrorSuccess
        || rangeValue == NULL
        || CFGetTypeID(rangeValue) != AXValueGetTypeID()
        || AXValueGetType((AXValueRef)rangeValue) != kAXValueCFRangeType) {
        if (rangeValue != NULL) CFRelease(rangeValue);
        return nil;
    }
    CFRange range = CFRangeMake(0, 0);
    bool valid = AXValueGetValue((AXValueRef)rangeValue, kAXValueCFRangeType, &range)
        && range.location >= 0
        && range.length > 0
        && (NSUInteger)range.location < value.length;
    CFRelease(rangeValue);
    if (!valid) return nil;
    NSUInteger location = (NSUInteger)range.location;
    NSUInteger length = MIN((NSUInteger)range.length, value.length - location);
    return length > 0 ? [value substringWithRange:NSMakeRange(location, length)] : nil;
}

static NSDictionary<NSString *, NSNumber *> *SayItAXSelectionBounds(AXUIElementRef element) {
    CFTypeRef rangeValue = NULL;
    AXError rangeError = AXUIElementCopyAttributeValue(
        element,
        kAXSelectedTextRangeAttribute,
        &rangeValue
    );
    if (rangeError != kAXErrorSuccess
        || rangeValue == NULL
        || CFGetTypeID(rangeValue) != AXValueGetTypeID()
        || AXValueGetType((AXValueRef)rangeValue) != kAXValueCFRangeType) {
        if (rangeValue != NULL) CFRelease(rangeValue);
        return nil;
    }
    CFRange range = CFRangeMake(0, 0);
    if (!AXValueGetValue((AXValueRef)rangeValue, kAXValueCFRangeType, &range) || range.length <= 0) {
        CFRelease(rangeValue);
        return nil;
    }
    CFTypeRef boundsValue = NULL;
    AXError boundsError = AXUIElementCopyParameterizedAttributeValue(
        element,
        kAXBoundsForRangeParameterizedAttribute,
        rangeValue,
        &boundsValue
    );
    CFRelease(rangeValue);
    if (boundsError != kAXErrorSuccess
        || boundsValue == NULL
        || CFGetTypeID(boundsValue) != AXValueGetTypeID()
        || AXValueGetType((AXValueRef)boundsValue) != kAXValueCGRectType) {
        if (boundsValue != NULL) CFRelease(boundsValue);
        return nil;
    }
    CGRect bounds = CGRectZero;
    bool valid = AXValueGetValue((AXValueRef)boundsValue, kAXValueCGRectType, &bounds)
        && isfinite(bounds.origin.x)
        && isfinite(bounds.origin.y)
        && isfinite(bounds.size.width)
        && isfinite(bounds.size.height)
        && bounds.size.width > 0
        && bounds.size.height > 0;
    CFRelease(boundsValue);
    if (!valid) return nil;
    return @{
        @"x": @(bounds.origin.x),
        @"y": @(bounds.origin.y),
        @"width": @(bounds.size.width),
        @"height": @(bounds.size.height),
    };
}

static NSNumber *SayItAXSelectionEditable(AXUIElementRef element) {
    CFTypeRef enabledValue = NULL;
    AXError enabledError = AXUIElementCopyAttributeValue(element, kAXEnabledAttribute, &enabledValue);
    if (enabledError == kAXErrorSuccess
        && enabledValue != NULL
        && CFGetTypeID(enabledValue) == CFBooleanGetTypeID()
        && !CFBooleanGetValue((CFBooleanRef)enabledValue)) {
        CFRelease(enabledValue);
        return @NO;
    }
    if (enabledValue != NULL) CFRelease(enabledValue);
    Boolean valueSettable = false;
    Boolean selectionSettable = false;
    AXError valueError = AXUIElementIsAttributeSettable(element, kAXValueAttribute, &valueSettable);
    AXError selectionError = AXUIElementIsAttributeSettable(
        element,
        kAXSelectedTextAttribute,
        &selectionSettable
    );
    if ((valueError == kAXErrorSuccess && valueSettable)
        || (selectionError == kAXErrorSuccess && selectionSettable)) return @YES;
    return nil;
}

int32_t sayit_macos_focused_input_editable(uint32_t processId, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"检查焦点输入区域需要辅助功能权限");
        return -1;
    }
    AXUIElementRef application = AXUIElementCreateApplication((pid_t)processId);
    AXUIElementSetMessagingTimeout(application, 0.15f);
    CFTypeRef focusedValue = NULL;
    AXError focusedError = AXUIElementCopyAttributeValue(
        application,
        kAXFocusedUIElementAttribute,
        &focusedValue
    );
    CFRelease(application);
    if (focusedError != kAXErrorSuccess || focusedValue == NULL) {
        if (focusedValue != NULL) CFRelease(focusedValue);
        SayItSetError(error, @"无法读取当前焦点控件");
        return -1;
    }
    AXUIElementRef focused = (AXUIElementRef)focusedValue;
    AXUIElementSetMessagingTimeout(focused, 0.15f);
    NSNumber *editable = SayItAXSelectionEditable(focused);
    CFRelease(focusedValue);
    if (editable == nil) return 0;
    return editable.boolValue ? 1 : 0;
}

static NSString *SayItAXSelectedTextInTree(
    AXUIElementRef element,
    NSUInteger depth,
    NSUInteger *remaining,
    AXUIElementRef *matchedElement
) {
    if (element == NULL || remaining == NULL || *remaining == 0) return nil;
    *remaining -= 1;
    NSString *selected = SayItAXSelectedTextFromElement(element);
    if (selected.length > 0) {
        if (matchedElement != NULL) *matchedElement = (AXUIElementRef)CFRetain(element);
        return selected;
    }
    if (depth == 0) return nil;

    CFTypeRef childrenValue = NULL;
    AXError childrenError = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute, &childrenValue);
    if (childrenError != kAXErrorSuccess || childrenValue == NULL || CFGetTypeID(childrenValue) != CFArrayGetTypeID()) {
        if (childrenValue != NULL) CFRelease(childrenValue);
        return nil;
    }
    CFArrayRef children = (CFArrayRef)childrenValue;
    for (CFIndex index = 0; index < CFArrayGetCount(children) && *remaining > 0; index += 1) {
        CFTypeRef child = CFArrayGetValueAtIndex(children, index);
        if (child == NULL || CFGetTypeID(child) != AXUIElementGetTypeID()) continue;
        AXUIElementSetMessagingTimeout((AXUIElementRef)child, 0.08f);
        selected = SayItAXSelectedTextInTree(
            (AXUIElementRef)child,
            depth - 1,
            remaining,
            matchedElement
        );
        if (selected.length > 0) break;
    }
    CFRelease(childrenValue);
    return selected;
}

static NSString *SayItAXSelectedTextWithAncestors(
    AXUIElementRef element,
    AXUIElementRef *matchedElement
) {
    AXUIElementRef current = (AXUIElementRef)CFRetain(element);
    NSString *selected = nil;
    for (NSUInteger depth = 0; current != NULL && depth < 10; depth += 1) {
        NSUInteger remaining = 96;
        selected = SayItAXSelectedTextInTree(current, 3, &remaining, matchedElement);
        if (selected.length > 0) break;
        CFTypeRef parentValue = NULL;
        AXError parentError = AXUIElementCopyAttributeValue(current, kAXParentAttribute, &parentValue);
        CFRelease(current);
        current = NULL;
        if (parentError != kAXErrorSuccess || parentValue == NULL || CFGetTypeID(parentValue) != AXUIElementGetTypeID()) {
            if (parentValue != NULL) CFRelease(parentValue);
            break;
        }
        current = (AXUIElementRef)parentValue;
        AXUIElementSetMessagingTimeout(current, 0.15f);
    }
    if (current != NULL) CFRelease(current);
    return selected;
}

static NSString *SayItTruncateAccessibilityText(NSString *text, NSUInteger maxLength) {
    if (text.length <= maxLength) return text;
    NSRange range = [text rangeOfComposedCharacterSequencesForRange:NSMakeRange(0, maxLength)];
    return [text substringWithRange:range];
}

static char *SayItAccessibilityContextJSON(
    uint32_t processId,
    uint32_t maxChars,
    char **error
) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"读取当前软件文本需要辅助功能权限");
        return NULL;
    }
    if (processId == 0) {
        SayItSetError(error, @"当前软件进程无效");
        return NULL;
    }

    AXUIElementRef application = AXUIElementCreateApplication((pid_t)processId);
    AXUIElementSetMessagingTimeout(application, 0.2f);
    CFTypeRef focusedValue = NULL;
    AXError focusedError = AXUIElementCopyAttributeValue(
        application,
        kAXFocusedUIElementAttribute,
        &focusedValue
    );
    if (focusedError != kAXErrorSuccess || focusedValue == NULL) {
        if (focusedValue != NULL) {
            CFRelease(focusedValue);
            focusedValue = NULL;
        }
        focusedError = AXUIElementCopyAttributeValue(
            application,
            kAXFocusedWindowAttribute,
            &focusedValue
        );
    }
    if (focusedError != kAXErrorSuccess || focusedValue == NULL) {
        CFRelease(application);
        SayItSetError(error, @"当前软件没有可读取的焦点文本区域或窗口");
        return NULL;
    }

    AXUIElementRef focused = (AXUIElementRef)focusedValue;
    AXUIElementSetMessagingTimeout(focused, 0.15f);
    CFTypeRef subrole = NULL;
    AXError subroleError = AXUIElementCopyAttributeValue(focused, kAXSubroleAttribute, &subrole);
    bool secure = subroleError == kAXErrorSuccess
        && subrole != NULL
        && CFGetTypeID(subrole) == CFStringGetTypeID()
        && CFEqual(subrole, kAXSecureTextFieldSubrole);
    if (subrole != NULL) CFRelease(subrole);
    if (secure) {
        CFRelease(application);
        CFRelease(focusedValue);
        return SayItCopyJSON(@{ @"secure": @YES }, error);
    }

    NSUInteger limit = MAX((NSUInteger)1, MIN((NSUInteger)maxChars, (NSUInteger)6000));
    // Chromium、表格和跨子节点选区经常只在祖先元素暴露 AXSelectedText。
    // 最多向上检查 10 层，与 selection-hook 的被动读取策略保持一致。
    AXUIElementRef selectedElement = NULL;
    NSString *selectedText = SayItAXSelectedTextWithAncestors(focused, &selectedElement);
    if (selectedText.length == 0) {
        // Chromium/Electron may not publish its accessibility tree until these
        // application attributes are enabled. selection-hook uses the same pair.
        AXUIElementSetAttributeValue(application, CFSTR("AXEnhancedUserInterface"), kCFBooleanTrue);
        AXUIElementSetAttributeValue(application, CFSTR("AXManualAccessibility"), kCFBooleanTrue);
        selectedText = SayItAXSelectedTextWithAncestors(focused, &selectedElement);
    }
    NSString *focusedText = SayItAXStringAttribute(focused, kAXValueAttribute);
    NSMutableDictionary *result = [NSMutableDictionary dictionary];
    if (selectedText.length > 0) {
        result[@"selectedText"] = SayItTruncateAccessibilityText(selectedText, limit);
        NSDictionary *bounds = SayItAXSelectionBounds(selectedElement ?: focused);
        if (bounds != nil) result[@"selectionBounds"] = bounds;
        NSNumber *editable = SayItAXSelectionEditable(selectedElement ?: focused);
        if (editable != nil) result[@"selectionEditable"] = editable;
    }
    if (focusedText.length > 0) {
        result[@"focusedText"] = SayItTruncateAccessibilityText(focusedText, limit);

        CFTypeRef rangeValue = NULL;
        AXError rangeError = AXUIElementCopyAttributeValue(
            focused,
            kAXSelectedTextRangeAttribute,
            &rangeValue
        );
        if (rangeError == kAXErrorSuccess
            && rangeValue != NULL
            && CFGetTypeID(rangeValue) == AXValueGetTypeID()
            && AXValueGetType((AXValueRef)rangeValue) == kAXValueCFRangeType) {
            CFRange selectedRange = CFRangeMake(0, 0);
            if (AXValueGetValue((AXValueRef)rangeValue, kAXValueCFRangeType, &selectedRange)
                && selectedRange.location >= 0
                && selectedRange.length >= 0) {
                NSUInteger location = MIN((NSUInteger)selectedRange.location, focusedText.length);
                NSUInteger availableLength = focusedText.length - location;
                NSUInteger selectionLength = MIN((NSUInteger)selectedRange.length, availableLength);
                NSUInteger selectionEnd = location + selectionLength;
                NSUInteger contextStart = location > 256 ? location - 256 : 0;
                NSUInteger contextEnd = MIN(focusedText.length, selectionEnd + 256);
                NSRange contextRange = NSMakeRange(contextStart, contextEnd - contextStart);
                contextRange = [focusedText rangeOfComposedCharacterSequencesForRange:contextRange];
                NSString *caretContext = [focusedText substringWithRange:contextRange];
                if (caretContext.length > 0) {
                    result[@"caretContext"] = SayItTruncateAccessibilityText(caretContext, limit);
                }
            }
        }
        if (rangeValue != NULL) CFRelease(rangeValue);
    }
    if (selectedElement != NULL) CFRelease(selectedElement);
    CFRelease(application);
    CFRelease(focusedValue);
    return SayItCopyJSON(result, error);
}

char *sayit_macos_accessibility_context_json(
    uint32_t processId,
    uint32_t maxChars,
    char **error
) {
    @autoreleasepool {
        return SayItAccessibilityContextJSON(processId, maxChars, error);
    }
}

static CGImageRef SayItCaptureWindowImage(CGWindowID windowId, uint32_t maxSide, NSString **message) {
    if (@available(macOS 14.0, *)) {
        __block SCShareableContent *content = nil;
        __block NSError *contentError = nil;
        dispatch_semaphore_t contentReady = dispatch_semaphore_create(0);
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES completionHandler:^(SCShareableContent *value, NSError *valueError) {
            content = value;
            contentError = valueError;
            dispatch_semaphore_signal(contentReady);
        }];
        if (dispatch_semaphore_wait(contentReady, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC)) != 0) {
            *message = @"等待 macOS 窗口截图权限超时";
            return NULL;
        }
        SCWindow *window = nil;
        for (SCWindow *candidate in content.windows) {
            if (candidate.windowID == windowId) {
                window = candidate;
                break;
            }
        }
        if (window == nil) {
            *message = contentError.localizedDescription ?: @"目标窗口不在 macOS 可共享内容中";
            return NULL;
        }
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDesktopIndependentWindow:window];
        double scale = 2.0;
        size_t sourceWidth = MAX((size_t)1, (size_t)llround(window.frame.size.width * scale));
        size_t sourceHeight = MAX((size_t)1, (size_t)llround(window.frame.size.height * scale));
        double outputScale = (maxSide > 0 && MAX(sourceWidth, sourceHeight) > maxSide)
            ? (double)maxSide / (double)MAX(sourceWidth, sourceHeight)
            : 1.0;
        SCStreamConfiguration *configuration = [[SCStreamConfiguration alloc] init];
        configuration.width = MAX((size_t)1, (size_t)llround(sourceWidth * outputScale));
        configuration.height = MAX((size_t)1, (size_t)llround(sourceHeight * outputScale));
        configuration.showsCursor = NO;
        __block CGImageRef captured = NULL;
        __block NSError *captureError = nil;
        dispatch_semaphore_t captureReady = dispatch_semaphore_create(0);
        [SCScreenshotManager captureImageWithFilter:filter configuration:configuration completionHandler:^(CGImageRef image, NSError *valueError) {
            if (image != NULL) captured = CGImageRetain(image);
            captureError = valueError;
            dispatch_semaphore_signal(captureReady);
        }];
        if (dispatch_semaphore_wait(captureReady, dispatch_time(DISPATCH_TIME_NOW, 10 * NSEC_PER_SEC)) != 0) {
            *message = @"等待 macOS 窗口截图超时";
            return NULL;
        }
        if (captured == NULL) *message = captureError.localizedDescription ?: @"macOS 未返回窗口截图";
        return captured;
    }
    typedef CGImageRef (*LegacyWindowCapture)(CGRect, CGWindowListOption, CGWindowID, CGWindowImageOption);
    static LegacyWindowCapture legacyCapture = NULL;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        legacyCapture = (LegacyWindowCapture)dlsym(RTLD_DEFAULT, "CGWindowListCreateImage");
    });
    if (legacyCapture == NULL) {
        *message = @"当前 macOS 版本没有可用的窗口截图接口";
        return NULL;
    }
    return legacyCapture(
        CGRectNull,
        kCGWindowListOptionIncludingWindow,
        windowId,
        kCGWindowImageBoundsIgnoreFraming | kCGWindowImageBestResolution
    );
}

bool sayit_macos_capture_window_png(uint32_t windowId, uint32_t maxSide, SayItByteBuffer *output, char **error) {
    if (output == NULL || windowId == 0) {
        SayItSetError(error, @"窗口截图参数无效");
        return false;
    }
    if (!SayItScreenCaptureAccess(false)) {
        SayItSetError(error, @"窗口 OCR 需要屏幕录制权限，请在系统设置 → 隐私与安全性 → 屏幕录制中允许当前运行的说吧！进程；授权后请完全退出并重新启动应用");
        return false;
    }
    @autoreleasepool {
        NSString *captureError = nil;
        CGImageRef image = SayItCaptureWindowImage((CGWindowID)windowId, maxSide, &captureError);
        if (image == NULL) {
            SayItSetError(error, captureError ?: @"macOS 未返回目标窗口截图，请检查屏幕录制权限");
            return false;
        }
        size_t sourceWidth = CGImageGetWidth(image);
        size_t sourceHeight = CGImageGetHeight(image);
        double scale = (maxSide > 0 && MAX(sourceWidth, sourceHeight) > maxSide)
            ? (double)maxSide / (double)MAX(sourceWidth, sourceHeight)
            : 1.0;
        size_t targetWidth = MAX((size_t)1, (size_t)llround(sourceWidth * scale));
        size_t targetHeight = MAX((size_t)1, (size_t)llround(sourceHeight * scale));
        CGColorSpaceRef colorSpace = CGColorSpaceCreateDeviceRGB();
        CGContextRef context = CGBitmapContextCreate(
            NULL, targetWidth, targetHeight, 8, targetWidth * 4, colorSpace,
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big
        );
        CGColorSpaceRelease(colorSpace);
        if (context == NULL) {
            CGImageRelease(image);
            SayItSetError(error, @"创建 macOS 缩放截图失败");
            return false;
        }
        CGContextSetInterpolationQuality(context, kCGInterpolationHigh);
        CGContextDrawImage(context, CGRectMake(0, 0, targetWidth, targetHeight), image);
        CGImageRelease(image);
        CGImageRef scaledImage = CGBitmapContextCreateImage(context);
        CGContextRelease(context);
        NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:scaledImage];
        CGImageRelease(scaledImage);
        NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
        if (png == nil || png.length == 0) {
            SayItSetError(error, @"编码 macOS 窗口截图失败");
            return false;
        }
        output->data = malloc(png.length);
        if (output->data == NULL) {
            SayItSetError(error, @"分配窗口截图内存失败");
            return false;
        }
        memcpy(output->data, png.bytes, png.length);
        output->length = png.length;
        output->width = (uint32_t)bitmap.pixelsWide;
        output->height = (uint32_t)bitmap.pixelsHigh;
        return true;
    }
}

char *sayit_macos_vision_ocr_png(const uint8_t *bytes, size_t length, char **error) {
    if (bytes == NULL || length == 0) {
        SayItSetError(error, @"OCR 图片为空");
        return NULL;
    }
    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:bytes length:length];
        NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:data];
        CGImageRef image = bitmap.CGImage;
        if (image == NULL) {
            SayItSetError(error, @"macOS Vision 无法解码 OCR 图片");
            return NULL;
        }
        VNRecognizeTextRequest *request = [[VNRecognizeTextRequest alloc] init];
        request.recognitionLevel = VNRequestTextRecognitionLevelAccurate;
        request.usesLanguageCorrection = YES;
        if (@available(macOS 13.0, *)) request.automaticallyDetectsLanguage = YES;
        VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
        NSError *visionError = nil;
        if (![handler performRequests:@[request] error:&visionError] && visionError != nil) {
            SayItSetError(error, [NSString stringWithFormat:@"macOS Vision OCR 失败：%@", visionError.localizedDescription]);
            return NULL;
        }
        NSMutableArray *blocks = [NSMutableArray array];
        for (VNRecognizedTextObservation *observation in request.results ?: @[]) {
            VNRecognizedText *candidate = [observation topCandidates:1].firstObject;
            if (candidate == nil || candidate.string.length == 0) continue;
            CGRect box = observation.boundingBox;
            [blocks addObject:@{
                @"text": candidate.string,
                @"confidence": @(candidate.confidence),
                @"left": @(box.origin.x),
                @"top": @(1.0 - CGRectGetMaxY(box)),
                @"right": @(CGRectGetMaxX(box)),
                @"bottom": @(1.0 - box.origin.y),
            }];
        }
        return SayItCopyJSON(blocks, error);
    }
}

@interface SayItKeyboardTap : NSObject
@property(nonatomic) SayItCapsLockCallback capsLockCallback;
@property(nonatomic) SayItFnKeyCallback fnKeyCallback;
@property(nonatomic) SayItEscapeCallback escapeCallback;
@property(nonatomic) void *context;
@property(nonatomic) CFMachPortRef tap;
@property(nonatomic) CFRunLoopRef runLoop;
@property(nonatomic) dispatch_semaphore_t ready;
@property(nonatomic, copy) NSString *startupError;
@property(nonatomic) bool monitorCapsLock;
@property(nonatomic) bool monitorFnKey;
@property(nonatomic) bool monitorEscape;
@property(nonatomic) io_connect_t hidConnection;
@property(nonatomic) IOHIDManagerRef hidManager;
@property(nonatomic) bool preservedCapsLockState;
@property(nonatomic) CFAbsoluteTime lastCapsLockEventAt;
@property(nonatomic) bool rawCapsLockPressed;
@property(nonatomic) bool rawCapsLockActive;
@property(nonatomic) bool fnKeyPressed;
@end

static void SayItHIDInputValueCallback(
    void *context,
    IOReturn result,
    void *sender,
    IOHIDValueRef value
) {
    (void)sender;
    if (result != kIOReturnSuccess || value == NULL || context == NULL) return;
    SayItKeyboardTap *owner = (__bridge SayItKeyboardTap *)context;
    IOHIDElementRef element = IOHIDValueGetElement(value);
    if (element == NULL
        || IOHIDElementGetUsagePage(element) != kHIDPage_KeyboardOrKeypad
        || IOHIDElementGetUsage(element) != kHIDUsage_KeyboardCapsLock) {
        return;
    }
    bool pressed = IOHIDValueGetIntegerValue(value) != 0;
    if (pressed && !owner.rawCapsLockPressed && owner.capsLockCallback != NULL) {
        CGEventFlags flags = CGEventSourceFlagsState(kCGEventSourceStateCombinedSessionState);
        owner.capsLockCallback(owner.context, (uint64_t)flags);
    }
    owner.rawCapsLockPressed = pressed;
}

static CGEventRef SayItKeyboardEventCallback(CGEventTapProxy proxy, CGEventType type, CGEventRef event, void *context) {
    (void)proxy;
    SayItKeyboardTap *owner = (__bridge SayItKeyboardTap *)context;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (owner.tap != NULL) CGEventTapEnable(owner.tap, true);
        return event;
    }
    int64_t keyCode = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
    if (owner.monitorFnKey && type == kCGEventFlagsChanged && keyCode == 63) {
        CGEventFlags flags = CGEventGetFlags(event);
        bool pressed = (flags & kCGEventFlagMaskSecondaryFn) != 0;
        if (pressed == owner.fnKeyPressed) return NULL;
        owner.fnKeyPressed = pressed;
        bool swallow = owner.fnKeyCallback != NULL
            && owner.fnKeyCallback(owner.context, pressed, (uint64_t)flags);
        return swallow ? NULL : event;
    }
    if (owner.monitorEscape && keyCode == 53 && (type == kCGEventKeyDown || type == kCGEventKeyUp)) {
        bool swallow = owner.escapeCallback != NULL
            && owner.escapeCallback(owner.context, type == kCGEventKeyDown);
        return swallow ? NULL : event;
    }
    if (!owner.monitorCapsLock || type != kCGEventFlagsChanged || keyCode != 57) return event;
    // Session 级事件过滤器收到 Caps Lock 时，系统锁定状态已经发生变化；仅返回 NULL
    // 只能阻止事件继续投递，不能把大小写和键盘灯恢复。这里显式写回绑定前的状态。
    if (owner.hidConnection != IO_OBJECT_NULL) {
        IOHIDSetModifierLockState(owner.hidConnection, kIOHIDCapsLockState, owner.preservedCapsLockState);
    }
    CFAbsoluteTime now = CFAbsoluteTimeGetCurrent();
    bool duplicated = owner.lastCapsLockEventAt > 0 && now - owner.lastCapsLockEventAt < 0.12;
    owner.lastCapsLockEventAt = now;
    bool swallow = true;
    if (!owner.rawCapsLockActive && !duplicated && owner.capsLockCallback != NULL) {
        swallow = owner.capsLockCallback(owner.context, (uint64_t)CGEventGetFlags(event));
    }
    return swallow ? NULL : event;
}

@implementation SayItKeyboardTap
- (void)startOnThread {
    @autoreleasepool {
        if (self.monitorCapsLock) {
            IOHIDAccessType access = IOHIDCheckAccess(kIOHIDRequestTypeListenEvent);
            if (access != kIOHIDAccessTypeGranted
                && !IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)) {
                self.startupError = @"快速响应 Caps Lock 需要“输入监控”权限；请在系统设置 → 隐私与安全性 → 输入监控中授权，随后重启说吧！";
                dispatch_semaphore_signal(self.ready);
                return;
            }
            self.hidManager = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
            if (self.hidManager == NULL) {
                self.startupError = @"无法创建 macOS 原始键盘监听器";
                dispatch_semaphore_signal(self.ready);
                return;
            }
            NSDictionary *deviceMatching = @{
                @kIOHIDDeviceUsagePageKey: @(kHIDPage_GenericDesktop),
                @kIOHIDDeviceUsageKey: @(kHIDUsage_GD_Keyboard),
            };
            NSDictionary *valueMatching = @{
                @kIOHIDElementUsagePageKey: @(kHIDPage_KeyboardOrKeypad),
                @kIOHIDElementUsageKey: @(kHIDUsage_KeyboardCapsLock),
            };
            IOHIDManagerSetDeviceMatching(self.hidManager, (__bridge CFDictionaryRef)deviceMatching);
            IOHIDManagerSetInputValueMatching(self.hidManager, (__bridge CFDictionaryRef)valueMatching);
            IOHIDManagerRegisterInputValueCallback(
                self.hidManager,
                SayItHIDInputValueCallback,
                (__bridge void *)self
            );
            IOHIDManagerScheduleWithRunLoop(
                self.hidManager,
                CFRunLoopGetCurrent(),
                kCFRunLoopCommonModes
            );
            IOReturn opened = IOHIDManagerOpen(self.hidManager, kIOHIDOptionsTypeNone);
            if (opened != kIOReturnSuccess) {
                self.startupError = @"无法打开 macOS 原始键盘监听器，请确认已授予输入监控权限";
                dispatch_semaphore_signal(self.ready);
                return;
            }
            self.rawCapsLockActive = true;
        }
        CGEventMask mask = 0;
        if (self.monitorCapsLock || self.monitorFnKey) mask |= CGEventMaskBit(kCGEventFlagsChanged);
        if (self.monitorEscape) {
            mask |= CGEventMaskBit(kCGEventKeyDown);
            mask |= CGEventMaskBit(kCGEventKeyUp);
        }
        self.tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
            mask,
            SayItKeyboardEventCallback,
            (__bridge void *)self
        );
        if (self.tap == NULL) {
            self.startupError = @"无法创建 macOS 键盘事件过滤器，请在系统设置中授予辅助功能权限";
            dispatch_semaphore_signal(self.ready);
            return;
        }
        self.runLoop = CFRunLoopGetCurrent();
        CFRetain(self.runLoop);
        CFRunLoopSourceRef source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, self.tap, 0);
        CFRunLoopAddSource(self.runLoop, source, kCFRunLoopCommonModes);
        CFRelease(source);
        CGEventTapEnable(self.tap, true);
        dispatch_semaphore_signal(self.ready);
        CFRunLoopRun();
    }
}
- (void)stop {
    if (self.tap != NULL) {
        CGEventTapEnable(self.tap, false);
        CFMachPortInvalidate(self.tap);
    }
    if (self.hidManager != NULL) {
        IOHIDManagerUnscheduleFromRunLoop(
            self.hidManager,
            self.runLoop ?: CFRunLoopGetCurrent(),
            kCFRunLoopCommonModes
        );
        IOHIDManagerClose(self.hidManager, kIOHIDOptionsTypeNone);
    }
    if (self.runLoop != NULL) CFRunLoopStop(self.runLoop);
}
- (void)dealloc {
    if (_tap != NULL) CFRelease(_tap);
    if (_runLoop != NULL) CFRelease(_runLoop);
    if (_hidManager != NULL) CFRelease(_hidManager);
    if (_hidConnection != IO_OBJECT_NULL) IOServiceClose(_hidConnection);
}
@end

static bool SayItOpenHIDSystem(io_connect_t *connection, bool *capsLockState, NSString **message) {
    // IOKit 明确约定 NULL 表示默认 main port；这样可同时支持 macOS 11，
    // 避免引用仅在 macOS 12 引入的同义常量 kIOMainPortDefault。
    io_service_t service = IOServiceGetMatchingService(MACH_PORT_NULL, IOServiceMatching(kIOHIDSystemClass));
    if (service == IO_OBJECT_NULL) {
        *message = @"无法连接 macOS 键盘状态服务";
        return false;
    }
    kern_return_t opened = IOServiceOpen(service, mach_task_self(), kIOHIDParamConnectType, connection);
    IOObjectRelease(service);
    if (opened != KERN_SUCCESS) {
        *message = @"无法打开 macOS 键盘状态服务";
        return false;
    }
    bool current = false;
    kern_return_t read = IOHIDGetModifierLockState(*connection, kIOHIDCapsLockState, &current);
    if (read != KERN_SUCCESS) {
        IOServiceClose(*connection);
        *connection = IO_OBJECT_NULL;
        *message = @"无法读取 macOS 大写锁定状态";
        return false;
    }
    kern_return_t writable = IOHIDSetModifierLockState(
        *connection,
        kIOHIDCapsLockState,
        current
    );
    if (writable != KERN_SUCCESS) {
        IOServiceClose(*connection);
        *connection = IO_OBJECT_NULL;
        *message = @"无法控制 macOS 大写锁定状态";
        return false;
    }
    *capsLockState = current;
    return true;
}

void *sayit_macos_keyboard_tap_start(
    SayItCapsLockCallback capsLockCallback,
    SayItFnKeyCallback fnKeyCallback,
    SayItEscapeCallback escapeCallback,
    void *context,
    bool monitorCapsLock,
    bool monitorFnKey,
    bool monitorEscape,
    char **error
) {
    if (!monitorCapsLock && !monitorFnKey && !monitorEscape) {
        SayItSetError(error, @"macOS 键盘监听没有指定目标按键");
        return NULL;
    }
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    if (!AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options)) {
        SayItSetError(error, @"监听 Caps Lock、Fn 或 Esc 需要辅助功能权限；授权后请重启说吧！");
        return NULL;
    }
    SayItKeyboardTap *owner = [[SayItKeyboardTap alloc] init];
    owner.capsLockCallback = capsLockCallback;
    owner.fnKeyCallback = fnKeyCallback;
    owner.escapeCallback = escapeCallback;
    owner.context = context;
    owner.monitorCapsLock = monitorCapsLock;
    owner.monitorFnKey = monitorFnKey;
    owner.monitorEscape = monitorEscape;
    owner.ready = dispatch_semaphore_create(0);
    owner.hidConnection = IO_OBJECT_NULL;
    if (monitorCapsLock) {
        NSString *hidError = nil;
        io_connect_t hidConnection = IO_OBJECT_NULL;
        bool preservedCapsLockState = false;
        if (!SayItOpenHIDSystem(&hidConnection, &preservedCapsLockState, &hidError)) {
            SayItSetError(error, hidError);
            return NULL;
        }
        owner.hidConnection = hidConnection;
        owner.preservedCapsLockState = preservedCapsLockState;
    }
    [NSThread detachNewThreadSelector:@selector(startOnThread) toTarget:owner withObject:nil];
    if (dispatch_semaphore_wait(owner.ready, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0) {
        SayItSetError(error, @"启动 macOS 键盘事件过滤器超时");
        return NULL;
    }
    if (owner.startupError != nil) {
        SayItSetError(error, owner.startupError);
        return NULL;
    }
    return (void *)CFBridgingRetain(owner);
}

void sayit_macos_keyboard_tap_stop(void *handle) {
    if (handle == NULL) return;
    SayItKeyboardTap *owner = CFBridgingRelease(handle);
    [owner stop];
}

@interface SayItMouseMonitor : NSObject
@property(nonatomic) SayItMouseMonitorCallback callback;
@property(nonatomic) void *context;
@property(nonatomic) CFMachPortRef tap;
@property(nonatomic) CFRunLoopRef runLoop;
@property(nonatomic) dispatch_semaphore_t ready;
@property(nonatomic, copy) NSString *startupError;
@end

static CGEventRef SayItMouseMonitorEventCallback(
    CGEventTapProxy proxy,
    CGEventType type,
    CGEventRef event,
    void *context
) {
    (void)proxy;
    SayItMouseMonitor *owner = (__bridge SayItMouseMonitor *)context;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (owner.tap != NULL) CGEventTapEnable(owner.tap, true);
        return event;
    }
    if (owner.callback == NULL) return event;
    CGPoint point = CGEventGetLocation(event);
    bool buttonDown = CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, kCGMouseButtonLeft)
        || CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, kCGMouseButtonRight)
        || CGEventSourceButtonState(kCGEventSourceStateCombinedSessionState, kCGMouseButtonCenter);
    owner.callback(
        owner.context,
        point.x,
        point.y,
        buttonDown,
        type == kCGEventLeftMouseDown,
        type == kCGEventLeftMouseUp
    );
    return event;
}

@implementation SayItMouseMonitor
- (void)startOnThread {
    @autoreleasepool {
        CGEventMask mask = CGEventMaskBit(kCGEventMouseMoved)
            | CGEventMaskBit(kCGEventLeftMouseDragged)
            | CGEventMaskBit(kCGEventRightMouseDragged)
            | CGEventMaskBit(kCGEventOtherMouseDragged)
            | CGEventMaskBit(kCGEventLeftMouseDown)
            | CGEventMaskBit(kCGEventLeftMouseUp)
            | CGEventMaskBit(kCGEventRightMouseDown)
            | CGEventMaskBit(kCGEventRightMouseUp)
            | CGEventMaskBit(kCGEventOtherMouseDown)
            | CGEventMaskBit(kCGEventOtherMouseUp);
        self.tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGTailAppendEventTap,
            kCGEventTapOptionListenOnly,
            mask,
            SayItMouseMonitorEventCallback,
            (__bridge void *)self
        );
        if (self.tap == NULL) {
            self.startupError = @"监听全局鼠标移动需要“输入监控”或“辅助功能”权限；授权后请重启说吧！";
            dispatch_semaphore_signal(self.ready);
            return;
        }
        self.runLoop = CFRunLoopGetCurrent();
        CFRetain(self.runLoop);
        CFRunLoopSourceRef source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, self.tap, 0);
        CFRunLoopAddSource(self.runLoop, source, kCFRunLoopCommonModes);
        CFRelease(source);
        CGEventTapEnable(self.tap, true);
        dispatch_semaphore_signal(self.ready);
        CFRunLoopRun();
    }
}
- (void)stop {
    if (self.tap != NULL) {
        CGEventTapEnable(self.tap, false);
        CFMachPortInvalidate(self.tap);
    }
    if (self.runLoop != NULL) CFRunLoopStop(self.runLoop);
}
- (void)dealloc {
    if (_tap != NULL) CFRelease(_tap);
    if (_runLoop != NULL) CFRelease(_runLoop);
}
@end

void *sayit_macos_mouse_monitor_start(
    SayItMouseMonitorCallback callback,
    void *context,
    char **error
) {
    if (callback == NULL) {
        SayItSetError(error, @"macOS 鼠标监听回调无效");
        return NULL;
    }
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    if (!AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options)) {
        SayItSetError(error, @"鼠标手势需要辅助功能权限；授权后请重启说吧！");
        return NULL;
    }
    SayItMouseMonitor *owner = [[SayItMouseMonitor alloc] init];
    owner.callback = callback;
    owner.context = context;
    owner.ready = dispatch_semaphore_create(0);
    [NSThread detachNewThreadSelector:@selector(startOnThread) toTarget:owner withObject:nil];
    if (dispatch_semaphore_wait(owner.ready, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0) {
        SayItSetError(error, @"启动 macOS 鼠标监听超时");
        return NULL;
    }
    if (owner.startupError != nil) {
        SayItSetError(error, owner.startupError);
        return NULL;
    }
    return (void *)CFBridgingRetain(owner);
}

void sayit_macos_mouse_monitor_stop(void *handle) {
    if (handle == NULL) return;
    SayItMouseMonitor *owner = CFBridgingRelease(handle);
    [owner stop];
}

API_AVAILABLE(macos(13.0))
@interface SayItSystemAudioCapture : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic) SayItAudioCallback callback;
@property(nonatomic) SayItAudioErrorCallback errorCallback;
@property(nonatomic) void *context;
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic) dispatch_queue_t sampleQueue;
@end

@implementation SayItSystemAudioCapture
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    (void)stream;
    NSString *message = error.localizedDescription ?: @"未知 ScreenCaptureKit 错误";
    dispatch_async(self.sampleQueue, ^{
        if (self.errorCallback != NULL) {
            self.errorCallback(self.context, message.UTF8String);
        }
    });
}

- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
    (void)stream;
    if (type != SCStreamOutputTypeAudio || !CMSampleBufferIsValid(sampleBuffer) || self.callback == NULL) return;
    CMAudioFormatDescriptionRef description = CMSampleBufferGetFormatDescription(sampleBuffer);
    const AudioStreamBasicDescription *format = description == NULL ? NULL : CMAudioFormatDescriptionGetStreamBasicDescription(description);
    if (format == NULL || format->mFormatID != kAudioFormatLinearPCM || format->mBitsPerChannel == 0) return;
    size_t listSize = 0;
    CMBlockBufferRef blockBuffer = NULL;
    OSStatus status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer, &listSize, NULL, 0, NULL, NULL,
        kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, &blockBuffer
    );
    if (status != noErr || listSize == 0) return;
    if (blockBuffer != NULL) {
        CFRelease(blockBuffer);
        blockBuffer = NULL;
    }
    AudioBufferList *list = malloc(listSize);
    if (list == NULL) {
        if (blockBuffer != NULL) CFRelease(blockBuffer);
        return;
    }
    status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
        sampleBuffer, &listSize, list, listSize, NULL, NULL,
        kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, &blockBuffer
    );
    if (status != noErr) {
        free(list);
        if (blockBuffer != NULL) CFRelease(blockBuffer);
        return;
    }
    const bool isFloat = (format->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    const bool isSigned = (format->mFormatFlags & kAudioFormatFlagIsSignedInteger) != 0;
    const bool nonInterleaved = (format->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    const size_t bytesPerSample = format->mBitsPerChannel / 8;
    const size_t channels = MAX((size_t)1, format->mChannelsPerFrame);
    const size_t frames = (size_t)CMSampleBufferGetNumSamples(sampleBuffer);
    float *mono = frames == 0 ? NULL : malloc(frames * sizeof(float));
    if (mono != NULL && ((isFloat && bytesPerSample == 4) || (isSigned && bytesPerSample == 2))) {
        for (size_t frame = 0; frame < frames; frame++) {
            double sum = 0;
            size_t seen = 0;
            if (nonInterleaved) {
                for (size_t channel = 0; channel < MIN(channels, list->mNumberBuffers); channel++) {
                    AudioBuffer buffer = list->mBuffers[channel];
                    if (frame * bytesPerSample + bytesPerSample > buffer.mDataByteSize) continue;
                    sum += isFloat ? ((float *)buffer.mData)[frame] : ((int16_t *)buffer.mData)[frame] / 32768.0;
                    seen++;
                }
            } else if (list->mNumberBuffers > 0) {
                AudioBuffer buffer = list->mBuffers[0];
                for (size_t channel = 0; channel < channels; channel++) {
                    size_t index = frame * channels + channel;
                    if (index * bytesPerSample + bytesPerSample > buffer.mDataByteSize) continue;
                    sum += isFloat ? ((float *)buffer.mData)[index] : ((int16_t *)buffer.mData)[index] / 32768.0;
                    seen++;
                }
            }
            mono[frame] = seen == 0 ? 0.0f : (float)(sum / seen);
        }
        self.callback(self.context, mono, frames);
    }
    free(mono);
    free(list);
    if (blockBuffer != NULL) CFRelease(blockBuffer);
}
@end

void *sayit_macos_system_audio_start(SayItAudioCallback callback, SayItAudioErrorCallback errorCallback, void *context, char **error) {
    if (@available(macOS 13.0, *)) {
        if (!SayItScreenCaptureAccess(true)) {
            SayItSetError(error, @"系统音频采集需要屏幕录制权限，请在系统设置 → 隐私与安全性 → 屏幕录制中允许当前运行的说吧！进程；授权后请完全退出并重新启动应用");
            return NULL;
        }
        __block SCShareableContent *content = nil;
        __block NSError *contentError = nil;
        dispatch_semaphore_t contentReady = dispatch_semaphore_create(0);
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES completionHandler:^(SCShareableContent *value, NSError *valueError) {
            content = value;
            contentError = valueError;
            dispatch_semaphore_signal(contentReady);
        }];
        if (dispatch_semaphore_wait(contentReady, dispatch_time(DISPATCH_TIME_NOW, 15 * NSEC_PER_SEC)) != 0) {
            SayItSetError(error, @"等待 macOS 系统音频授权超时");
            return NULL;
        }
        if (content == nil || content.displays.count == 0) {
            SayItSetError(error, [NSString stringWithFormat:@"无法读取可共享屏幕：%@", contentError.localizedDescription ?: @"请检查屏幕与系统音频录制权限"]);
            return NULL;
        }
        pid_t ownPid = NSProcessInfo.processInfo.processIdentifier;
        NSPredicate *ownPredicate = [NSPredicate predicateWithBlock:^BOOL(SCRunningApplication *application, NSDictionary *bindings) {
            (void)bindings;
            return application.processID == ownPid;
        }];
        NSArray<SCRunningApplication *> *excludedApps = [content.applications filteredArrayUsingPredicate:ownPredicate];
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:content.displays.firstObject excludingApplications:excludedApps exceptingWindows:@[]];
        SCStreamConfiguration *configuration = [[SCStreamConfiguration alloc] init];
        configuration.capturesAudio = YES;
        configuration.excludesCurrentProcessAudio = YES;
        configuration.sampleRate = 16000;
        configuration.channelCount = 1;
        configuration.width = 2;
        configuration.height = 2;
        configuration.queueDepth = 3;

        SayItSystemAudioCapture *capture = [[SayItSystemAudioCapture alloc] init];
        capture.callback = callback;
        capture.errorCallback = errorCallback;
        capture.context = context;
        capture.sampleQueue = dispatch_queue_create("com.henji.sayit.system-audio", DISPATCH_QUEUE_SERIAL);
        capture.stream = [[SCStream alloc] initWithFilter:filter configuration:configuration delegate:capture];
        NSError *outputError = nil;
        if (![capture.stream addStreamOutput:capture type:SCStreamOutputTypeAudio sampleHandlerQueue:capture.sampleQueue error:&outputError]) {
            SayItSetError(error, [NSString stringWithFormat:@"注册系统音频输出失败：%@", outputError.localizedDescription]);
            return NULL;
        }
        __block NSError *startError = nil;
        dispatch_semaphore_t started = dispatch_semaphore_create(0);
        [capture.stream startCaptureWithCompletionHandler:^(NSError *valueError) {
            startError = valueError;
            dispatch_semaphore_signal(started);
        }];
        if (dispatch_semaphore_wait(started, dispatch_time(DISPATCH_TIME_NOW, 15 * NSEC_PER_SEC)) != 0) {
            SayItSetError(error, @"启动 macOS 系统音频采集超时");
            return NULL;
        }
        if (startError != nil) {
            SayItSetError(error, [NSString stringWithFormat:@"启动 macOS 系统音频采集失败：%@", startError.localizedDescription]);
            return NULL;
        }
        return (void *)CFBridgingRetain(capture);
    }
    SayItSetError(error, @"系统音频采集需要 macOS 13 或更高版本");
    return NULL;
}

void sayit_macos_system_audio_stop(void *handle) {
    if (handle == NULL) return;
    if (@available(macOS 13.0, *)) {
        SayItSystemAudioCapture *capture = CFBridgingRelease(handle);
        // Rust 会在本函数返回后释放 callback context。先在串行采样队列上做屏障并
        // 断开回调，保证已经入队或正在执行的回调全部结束，后续样本也不会再访问该指针。
        if (capture.sampleQueue != nil) {
            dispatch_sync(capture.sampleQueue, ^{
                capture.callback = NULL;
                capture.errorCallback = NULL;
                capture.context = NULL;
            });
        }
        dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
        [capture.stream stopCaptureWithCompletionHandler:^(NSError *error) {
            (void)error;
            dispatch_semaphore_signal(stopped);
        }];
        dispatch_semaphore_wait(stopped, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    }
}
