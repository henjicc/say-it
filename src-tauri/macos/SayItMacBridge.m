#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <AudioToolbox/AudioToolbox.h>
#import <CoreMedia/CoreMedia.h>
#import <IOKit/IOKitLib.h>
#import <IOKit/hidsystem/IOHIDLib.h>
#import <IOKit/hidsystem/IOHIDParameter.h>
#import <IOKit/hidsystem/IOHIDShared.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Vision/Vision.h>
#import <dlfcn.h>

typedef bool (*SayItCapsLockCallback)(void *context, uint64_t flags);
typedef void (*SayItAudioCallback)(void *context, const float *samples, size_t count);

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

bool sayit_macos_send_paste_shortcut(char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"模拟粘贴需要辅助功能权限");
        return false;
    }

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

static void SayItRunOnMainThread(dispatch_block_t block) {
    if (NSThread.isMainThread) {
        block();
    } else {
        dispatch_sync(dispatch_get_main_queue(), block);
    }
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

int32_t sayit_macos_focused_input_security(uint32_t processId, char **error) {
    if (!AXIsProcessTrusted()) {
        SayItSetError(error, @"检查密码输入区域需要辅助功能权限");
        return -1;
    }
    AXUIElementRef application = AXUIElementCreateApplication((pid_t)processId);
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

@interface SayItCapsTap : NSObject
@property(nonatomic) SayItCapsLockCallback callback;
@property(nonatomic) void *context;
@property(nonatomic) CFMachPortRef tap;
@property(nonatomic) CFRunLoopRef runLoop;
@property(nonatomic) dispatch_semaphore_t ready;
@property(nonatomic, copy) NSString *startupError;
@property(nonatomic) io_connect_t hidConnection;
@property(nonatomic) bool preservedCapsLockState;
@property(nonatomic) CFAbsoluteTime lastCapsLockEventAt;
@end

static CGEventRef SayItCapsEventCallback(CGEventTapProxy proxy, CGEventType type, CGEventRef event, void *context) {
    (void)proxy;
    SayItCapsTap *owner = (__bridge SayItCapsTap *)context;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (owner.tap != NULL) CGEventTapEnable(owner.tap, true);
        return event;
    }
    if (type != kCGEventFlagsChanged || CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) != 57) return event;
    // Session 级事件过滤器收到 Caps Lock 时，系统锁定状态已经发生变化；仅返回 NULL
    // 只能阻止事件继续投递，不能把大小写和键盘灯恢复。这里显式写回绑定前的状态。
    if (owner.hidConnection != IO_OBJECT_NULL) {
        IOHIDSetModifierLockState(owner.hidConnection, kIOHIDCapsLockState, owner.preservedCapsLockState);
    }
    CFAbsoluteTime now = CFAbsoluteTimeGetCurrent();
    bool duplicated = owner.lastCapsLockEventAt > 0 && now - owner.lastCapsLockEventAt < 0.12;
    owner.lastCapsLockEventAt = now;
    bool swallow = true;
    if (!duplicated && owner.callback != NULL) {
        swallow = owner.callback(owner.context, (uint64_t)CGEventGetFlags(event));
    }
    return swallow ? NULL : event;
}

@implementation SayItCapsTap
- (void)startOnThread {
    @autoreleasepool {
        CGEventMask mask = CGEventMaskBit(kCGEventFlagsChanged);
        self.tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
            mask,
            SayItCapsEventCallback,
            (__bridge void *)self
        );
        if (self.tap == NULL) {
            self.startupError = @"无法创建 Caps Lock 事件过滤器，请在系统设置中授予辅助功能权限";
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

void *sayit_macos_caps_lock_start(SayItCapsLockCallback callback, void *context, char **error) {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    if (!AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options)) {
        SayItSetError(error, @"使用 Caps Lock 快捷键需要辅助功能权限；授权后请重启说吧！");
        return NULL;
    }
    SayItCapsTap *owner = [[SayItCapsTap alloc] init];
    owner.callback = callback;
    owner.context = context;
    owner.ready = dispatch_semaphore_create(0);
    NSString *hidError = nil;
    io_connect_t hidConnection = IO_OBJECT_NULL;
    bool preservedCapsLockState = false;
    if (!SayItOpenHIDSystem(&hidConnection, &preservedCapsLockState, &hidError)) {
        SayItSetError(error, hidError);
        return NULL;
    }
    owner.hidConnection = hidConnection;
    owner.preservedCapsLockState = preservedCapsLockState;
    [NSThread detachNewThreadSelector:@selector(startOnThread) toTarget:owner withObject:nil];
    if (dispatch_semaphore_wait(owner.ready, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0) {
        SayItSetError(error, @"启动 Caps Lock 事件过滤器超时");
        return NULL;
    }
    if (owner.startupError != nil) {
        SayItSetError(error, owner.startupError);
        return NULL;
    }
    return (void *)CFBridgingRetain(owner);
}

void sayit_macos_caps_lock_stop(void *handle) {
    if (handle == NULL) return;
    SayItCapsTap *owner = CFBridgingRelease(handle);
    [owner stop];
}

API_AVAILABLE(macos(13.0))
@interface SayItSystemAudioCapture : NSObject <SCStreamOutput, SCStreamDelegate>
@property(nonatomic) SayItAudioCallback callback;
@property(nonatomic) void *context;
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic) dispatch_queue_t sampleQueue;
@end

@implementation SayItSystemAudioCapture
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

void *sayit_macos_system_audio_start(SayItAudioCallback callback, void *context, char **error) {
    if (@available(macOS 13.0, *)) {
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
        dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
        [capture.stream stopCaptureWithCompletionHandler:^(NSError *error) {
            (void)error;
            dispatch_semaphore_signal(stopped);
        }];
        dispatch_semaphore_wait(stopped, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    }
}
