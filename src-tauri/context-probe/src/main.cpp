#include <windows.h>
#include <objbase.h>
#include <oleauto.h>
#include <UIAutomationClient.h>
#include <oleacc.h>
#include <servprov.h>
#include <sddl.h>
#include <shellapi.h>

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cwctype>
#include <limits>
#include <optional>
#include <sstream>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

#include "ia2_text.h"

namespace {

using Clock = std::chrono::steady_clock;
constexpr DWORD kPipeBufferBytes = 128 * 1024;
constexpr size_t kMaxFrameBytes = 128 * 1024;
constexpr size_t kMinimumUsefulChars = 30;
constexpr int kMaxAncestorDepth = 10;
constexpr LONG kObjIdNativeOm = -16;

template <typename T>
class ComPtr {
public:
    ComPtr() = default;
    explicit ComPtr(T* value) : value_(value) {}
    ComPtr(const ComPtr&) = delete;
    ComPtr& operator=(const ComPtr&) = delete;
    ComPtr(ComPtr&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
    ComPtr& operator=(ComPtr&& other) noexcept {
        if (this != &other) {
            reset();
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~ComPtr() { reset(); }
    T* get() const { return value_; }
    T** put() {
        reset();
        return &value_;
    }
    T* operator->() const { return value_; }
    explicit operator bool() const { return value_ != nullptr; }
    void reset(T* next = nullptr) {
        if (value_) value_->Release();
        value_ = next;
    }
private:
    T* value_ = nullptr;
};

struct Request {
    uint32_t protocolVersion = 0;
    uint64_t requestId = 0;
    int64_t hwnd = 0;
    uint32_t pid = 0;
    size_t maxChars = 3000;
    bool deepClipboard = true;
    uint32_t readerBudgetMs = 650;
    bool hasCursor = false;
    POINT cursor{};
};

struct Result {
    uint64_t requestId = 0;
    std::string status = "empty";
    std::string source;
    std::wstring selectedText;
    std::wstring focusedText;
    std::wstring caretContext;
    std::vector<std::wstring> visibleText;
    std::vector<std::wstring> documentText;
    std::vector<std::wstring> diagnostics;
    uint64_t elapsedMs = 0;
    bool truncated = false;
};

struct DocumentCache {
    HWND hwnd = nullptr;
    std::wstring title;
    std::vector<std::wstring> text;
    Clock::time_point expires;
};

ComPtr<IUIAutomation> gAutomation;
struct ReaderStats {
    std::string source;
    double averageMs = 0;
    uint64_t samples = 0;
};

std::unordered_map<std::wstring, ReaderStats> gReaderStats;
std::unordered_map<std::wstring, DocumentCache> gDocumentCache;

std::wstring trim(const std::wstring& value) {
    size_t begin = 0;
    while (begin < value.size() && std::iswspace(value[begin])) ++begin;
    size_t end = value.size();
    while (end > begin && std::iswspace(value[end - 1])) --end;
    std::wstring output;
    output.reserve(end - begin);
    bool spacing = false;
    for (size_t i = begin; i < end; ++i) {
        wchar_t ch = value[i];
        if (ch == 0xfffc) continue;
        if (std::iswspace(ch)) {
            spacing = !output.empty();
        } else {
            if (spacing) output.push_back(L' ');
            output.push_back(ch);
            spacing = false;
        }
    }
    return output;
}

std::string utf8(const std::wstring& value) {
    if (value.empty()) return {};
    int length = WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), nullptr, 0, nullptr, nullptr);
    if (length <= 0) return {};
    std::string output(static_cast<size_t>(length), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), output.data(), length, nullptr, nullptr);
    return output;
}

std::string jsonEscape(const std::wstring& value) {
    const std::string input = utf8(value);
    std::ostringstream output;
    for (unsigned char ch : input) {
        switch (ch) {
            case '\\': output << "\\\\"; break;
            case '"': output << "\\\""; break;
            case '\b': output << "\\b"; break;
            case '\f': output << "\\f"; break;
            case '\n': output << "\\n"; break;
            case '\r': output << "\\r"; break;
            case '\t': output << "\\t"; break;
            default:
                if (ch < 0x20) {
                    const char hex[] = "0123456789abcdef";
                    output << "\\u00" << hex[(ch >> 4) & 0xf] << hex[ch & 0xf];
                } else {
                    output << static_cast<char>(ch);
                }
        }
    }
    return output.str();
}

std::optional<uint64_t> jsonUnsigned(const std::string& json, const char* key) {
    const std::string marker = std::string("\"") + key + "\":";
    size_t position = json.find(marker);
    if (position == std::string::npos) return std::nullopt;
    position += marker.size();
    while (position < json.size() && json[position] == ' ') ++position;
    size_t end = position;
    while (end < json.size() && json[end] >= '0' && json[end] <= '9') ++end;
    if (end == position) return std::nullopt;
    try { return std::stoull(json.substr(position, end - position)); }
    catch (...) { return std::nullopt; }
}

std::optional<int64_t> jsonSigned(const std::string& json, const char* key) {
    const std::string marker = std::string("\"") + key + "\":";
    size_t position = json.find(marker);
    if (position == std::string::npos) return std::nullopt;
    position += marker.size();
    while (position < json.size() && json[position] == ' ') ++position;
    size_t end = position;
    if (end < json.size() && json[end] == '-') ++end;
    const size_t digits = end;
    while (end < json.size() && json[end] >= '0' && json[end] <= '9') ++end;
    if (end == digits) return std::nullopt;
    try { return std::stoll(json.substr(position, end - position)); }
    catch (...) { return std::nullopt; }
}

bool jsonBool(const std::string& json, const char* key, bool fallback) {
    const std::string marker = std::string("\"") + key + "\":";
    size_t position = json.find(marker);
    if (position == std::string::npos) return fallback;
    position += marker.size();
    while (position < json.size() && json[position] == ' ') ++position;
    if (json.compare(position, 4, "true") == 0) return true;
    if (json.compare(position, 5, "false") == 0) return false;
    return fallback;
}

std::optional<Request> parseRequest(const std::string& json) {
    Request request;
    auto protocol = jsonUnsigned(json, "protocolVersion");
    auto requestId = jsonUnsigned(json, "requestId");
    auto hwnd = jsonUnsigned(json, "hwnd");
    auto pid = jsonUnsigned(json, "pid");
    auto maxChars = jsonUnsigned(json, "maxChars");
    auto readerBudgetMs = jsonUnsigned(json, "readerBudgetMs");
    auto cursorX = jsonSigned(json, "cursorX");
    auto cursorY = jsonSigned(json, "cursorY");
    if (!protocol || !requestId || !hwnd || !pid || !maxChars) return std::nullopt;
    request.protocolVersion = static_cast<uint32_t>(*protocol);
    request.requestId = *requestId;
    request.hwnd = static_cast<int64_t>(*hwnd);
    request.pid = static_cast<uint32_t>(*pid);
    request.maxChars = std::clamp<size_t>(static_cast<size_t>(*maxChars), 1, 6000);
    request.deepClipboard = jsonBool(json, "deepClipboard", true);
    request.readerBudgetMs = static_cast<uint32_t>(std::clamp<uint64_t>(readerBudgetMs.value_or(650), 50, 700));
    if (cursorX && cursorY
        && *cursorX >= std::numeric_limits<LONG>::min() && *cursorX <= std::numeric_limits<LONG>::max()
        && *cursorY >= std::numeric_limits<LONG>::min() && *cursorY <= std::numeric_limits<LONG>::max()) {
        request.hasCursor = true;
        request.cursor.x = static_cast<LONG>(*cursorX);
        request.cursor.y = static_cast<LONG>(*cursorY);
    }
    return request;
}

void appendJsonArray(std::ostringstream& json, const std::vector<std::wstring>& values) {
    json << '[';
    for (size_t i = 0; i < values.size(); ++i) {
        if (i) json << ',';
        json << '"' << jsonEscape(values[i]) << '"';
    }
    json << ']';
}

std::string serializeResult(const Result& result) {
    std::ostringstream json;
    json << "{\"protocolVersion\":1,\"requestId\":" << result.requestId
         << ",\"status\":\"" << result.status << "\",\"source\":\"" << result.source
         << "\",\"selectedText\":\"" << jsonEscape(result.selectedText)
         << "\",\"focusedText\":\"" << jsonEscape(result.focusedText)
         << "\",\"caretContext\":\"" << jsonEscape(result.caretContext)
         << "\",\"visibleText\":";
    appendJsonArray(json, result.visibleText);
    json << ",\"documentText\":";
    appendJsonArray(json, result.documentText);
    json << ",\"diagnostics\":";
    appendJsonArray(json, result.diagnostics);
    json << ",\"elapsedMs\":" << result.elapsedMs
         << ",\"truncated\":" << (result.truncated ? "true" : "false") << '}';
    return json.str();
}

bool readExact(HANDLE pipe, void* buffer, DWORD length) {
    auto* bytes = static_cast<unsigned char*>(buffer);
    DWORD completed = 0;
    while (completed < length) {
        DWORD read = 0;
        if (!ReadFile(pipe, bytes + completed, length - completed, &read, nullptr) || read == 0) return false;
        completed += read;
    }
    return true;
}

bool writeExact(HANDLE pipe, const void* buffer, DWORD length) {
    const auto* bytes = static_cast<const unsigned char*>(buffer);
    DWORD completed = 0;
    while (completed < length) {
        DWORD written = 0;
        if (!WriteFile(pipe, bytes + completed, length - completed, &written, nullptr) || written == 0) return false;
        completed += written;
    }
    return true;
}

std::wstring processName(DWORD pid) {
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) return {};
    std::wstring path(32768, L'\0');
    DWORD length = static_cast<DWORD>(path.size());
    if (!QueryFullProcessImageNameW(process, 0, path.data(), &length)) length = 0;
    CloseHandle(process);
    path.resize(length);
    size_t slash = path.find_last_of(L"\\/");
    std::wstring name = slash == std::wstring::npos ? path : path.substr(slash + 1);
    std::transform(name.begin(), name.end(), name.begin(), std::towlower);
    return name;
}

std::wstring windowTitle(HWND hwnd) {
    int length = GetWindowTextLengthW(hwnd);
    if (length <= 0) return {};
    std::wstring title(static_cast<size_t>(length) + 1, L'\0');
    int copied = GetWindowTextW(hwnd, title.data(), length + 1);
    title.resize(copied > 0 ? static_cast<size_t>(copied) : 0);
    return trim(title);
}

bool sameExecutable(DWORD candidate, DWORD target, const std::wstring& targetName) {
    return candidate == target || (!targetName.empty() && processName(candidate) == targetName);
}

size_t textSize(const Result& result) {
    size_t size = result.selectedText.size() + result.focusedText.size() + result.caretContext.size();
    for (const auto& value : result.visibleText) size += value.size();
    for (const auto& value : result.documentText) size += value.size();
    return size;
}

void pushUnique(std::vector<std::wstring>& output, const std::wstring& raw) {
    std::wstring value = trim(raw);
    if (value.empty()) return;
    if (std::find(output.begin(), output.end(), value) == output.end()) output.push_back(std::move(value));
}

std::wstring rangeText(IUIAutomationTextRange* range, int limit) {
    if (!range) return {};
    BSTR text = nullptr;
    if (FAILED(range->GetText(limit, &text)) || !text) return {};
    std::wstring value(text, SysStringLen(text));
    SysFreeString(text);
    return trim(value);
}

bool elementIsPassword(IUIAutomationElement* element) {
    if (!element) return false;
    BOOL value = FALSE;
    return SUCCEEDED(element->get_CurrentIsPassword(&value)) && value == TRUE;
}

bool focusMatchesTarget(IUIAutomationElement* element, DWORD pid, const std::wstring& targetName) {
    if (!element) return false;
    int focusedPid = 0;
    return SUCCEEDED(element->get_CurrentProcessId(&focusedPid))
        && focusedPid > 0
        && sameExecutable(static_cast<DWORD>(focusedPid), pid, targetName);
}

bool readIa2Accessible(IAccessible* accessible, Result& result, size_t maxChars, const POINT* point = nullptr) {
    if (!accessible) return false;
    ComPtr<IAccessibleText> text;
    ComPtr<IServiceProvider> service;
    if (SUCCEEDED(accessible->QueryInterface(IID_PPV_ARGS(service.put()))) && service) {
        service->QueryService(__uuidof(IAccessible), __uuidof(IAccessibleText), reinterpret_cast<void**>(text.put()));
    }
    if (!text) accessible->QueryInterface(__uuidof(IAccessibleText), reinterpret_cast<void**>(text.put()));
    if (!text) return false;

    long selections = 0;
    if (SUCCEEDED(text->get_nSelections(&selections)) && selections > 0) {
        long start = 0, end = 0;
        if (SUCCEEDED(text->get_selection(0, &start, &end)) && end > start) {
            BSTR selected = nullptr;
            if (SUCCEEDED(text->get_text(start, end, &selected)) && selected) {
                result.selectedText = trim(std::wstring(selected, SysStringLen(selected)));
                SysFreeString(selected);
            }
        }
    }
    long count = 0;
    if (FAILED(text->get_nCharacters(&count)) || count <= 0) return !result.selectedText.empty();
    long caret = -1;
    const bool hasCaret = SUCCEEDED(text->get_caretOffset(&caret)) && caret >= 0;
    if (!hasCaret && point) {
        // 浏览器中的焦点有时落在容器；IA2 的定点偏移可直接回到用户刚点击的文本位置。
        text->get_offsetAtPoint(point->x, point->y, IA2_COORDTYPE_SCREEN_RELATIVE, &caret);
    }
    if (caret >= 0) {
        long start = std::max<long>(0, caret - 500);
        long end = std::min<long>(count, caret + 500);
        BSTR context = nullptr;
        if (SUCCEEDED(text->get_text(start, end, &context)) && context) {
            result.caretContext = trim(std::wstring(context, SysStringLen(context)));
            SysFreeString(context);
        }
    }
    const long limit = static_cast<long>(std::min<size_t>(maxChars, static_cast<size_t>(count)));
    BSTR document = nullptr;
    if (SUCCEEDED(text->get_text(0, limit, &document)) && document) {
        pushUnique(result.documentText, std::wstring(document, SysStringLen(document)));
        SysFreeString(document);
        result.truncated |= count > limit;
    }
    if (textSize(result) > 0) result.source = "ia2Text";
    return textSize(result) > 0;
}

bool readIa2(IUIAutomationElement* element, Result& result, size_t maxChars) {
    if (!element) return false;
    ComPtr<IUIAutomationLegacyIAccessiblePattern> legacy;
    if (FAILED(element->GetCurrentPatternAs(UIA_LegacyIAccessiblePatternId,
            __uuidof(IUIAutomationLegacyIAccessiblePattern), reinterpret_cast<void**>(legacy.put()))) || !legacy) {
        return false;
    }
    ComPtr<IAccessible> accessible;
    if (FAILED(legacy->GetIAccessible(accessible.put())) || !accessible) return false;
    return readIa2Accessible(accessible.get(), result, maxChars);
}

bool readTextPattern(IUIAutomationElement* element, Result& result, size_t maxChars, bool documentFallback) {
    if (!element) return false;
    ComPtr<IUIAutomationTextPattern> pattern;
    if (FAILED(element->GetCurrentPatternAs(UIA_TextPatternId, __uuidof(IUIAutomationTextPattern), reinterpret_cast<void**>(pattern.put()))) || !pattern) {
        return false;
    }

    ComPtr<IUIAutomationTextRangeArray> selections;
    if (SUCCEEDED(pattern->GetSelection(selections.put())) && selections) {
        int count = 0;
        if (SUCCEEDED(selections->get_Length(&count)) && count > 0) {
            ComPtr<IUIAutomationTextRange> range;
            if (SUCCEEDED(selections->GetElement(0, range.put()))) {
                std::wstring selected = rangeText(range.get(), static_cast<int>(maxChars));
                if (!selected.empty()) result.selectedText = std::move(selected);
            }
        }
    }

    ComPtr<IUIAutomationTextPattern2> pattern2;
    if (SUCCEEDED(element->GetCurrentPatternAs(UIA_TextPattern2Id, __uuidof(IUIAutomationTextPattern2), reinterpret_cast<void**>(pattern2.put()))) && pattern2) {
        BOOL active = FALSE;
        ComPtr<IUIAutomationTextRange> caret;
        if (SUCCEEDED(pattern2->GetCaretRange(&active, caret.put())) && caret) {
            caret->ExpandToEnclosingUnit(TextUnit_Line);
            result.caretContext = rangeText(caret.get(), 1000);
        }
    }

    ComPtr<IUIAutomationValuePattern> valuePattern;
    if (SUCCEEDED(element->GetCurrentPatternAs(UIA_ValuePatternId, __uuidof(IUIAutomationValuePattern), reinterpret_cast<void**>(valuePattern.put()))) && valuePattern) {
        BSTR value = nullptr;
        if (SUCCEEDED(valuePattern->get_CurrentValue(&value)) && value) {
            result.focusedText = trim(std::wstring(value, SysStringLen(value)));
            SysFreeString(value);
        }
    }

    if (documentFallback) {
        ComPtr<IUIAutomationTextRangeArray> visible;
        if (SUCCEEDED(pattern->GetVisibleRanges(visible.put())) && visible) {
            int count = 0;
            if (SUCCEEDED(visible->get_Length(&count))) {
                for (int index = 0; index < std::min(count, 4); ++index) {
                    ComPtr<IUIAutomationTextRange> range;
                    if (SUCCEEDED(visible->GetElement(index, range.put()))) {
                        pushUnique(result.visibleText, rangeText(range.get(), static_cast<int>(maxChars)));
                    }
                }
            }
        }
        if (result.visibleText.empty()) {
            ComPtr<IUIAutomationTextRange> document;
            if (SUCCEEDED(pattern->get_DocumentRange(document.put())) && document) {
                pushUnique(result.documentText, rangeText(document.get(), static_cast<int>(maxChars)));
            }
        }
    }
    if (textSize(result) > 0) result.source = "uiaTextPattern";
    return textSize(result) > 0;
}

ComPtr<IUIAutomationElement> focusedElement(HWND target, DWORD pid, const std::wstring& targetName) {
    if (!gAutomation) return {};
    ComPtr<IUIAutomationElement> focused;
    if (FAILED(gAutomation->GetFocusedElement(focused.put())) || !focused) return {};
    if (!focusMatchesTarget(focused.get(), pid, targetName)) return {};
    (void)target;
    return focused;
}

bool pointBelongsToTarget(HWND target, POINT point) {
    HWND hit = WindowFromPoint(point);
    if (!hit) return false;
    return GetAncestor(hit, GA_ROOT) == target;
}

ComPtr<IUIAutomationElement> elementAtPoint(POINT point, DWORD pid, const std::wstring& targetName) {
    if (!gAutomation) return {};
    ComPtr<IUIAutomationElement> element;
    if (FAILED(gAutomation->ElementFromPoint(point, element.put())) || !element) return {};
    return focusMatchesTarget(element.get(), pid, targetName) ? std::move(element) : ComPtr<IUIAutomationElement>{};
}

ComPtr<IAccessible> accessibleAtPoint(POINT point) {
    ComPtr<IAccessible> parent;
    VARIANT child;
    VariantInit(&child);
    if (FAILED(AccessibleObjectFromPoint(point, parent.put(), &child)) || !parent) {
        VariantClear(&child);
        return {};
    }
    // 定点 API 可能返回父级对象和一个子项 ID；若实际子项可取得 IAccessible，优先它。
    if (child.vt == VT_DISPATCH && child.pdispVal) {
        ComPtr<IAccessible> actual;
        if (SUCCEEDED(child.pdispVal->QueryInterface(IID_PPV_ARGS(actual.put()))) && actual) {
            VariantClear(&child);
            return actual;
        }
    }
    if (child.vt == VT_I4 && child.lVal != CHILDID_SELF) {
        ComPtr<IDispatch> childDispatch;
        if (SUCCEEDED(parent->get_accChild(child, childDispatch.put())) && childDispatch) {
            ComPtr<IAccessible> actual;
            if (SUCCEEDED(childDispatch->QueryInterface(IID_PPV_ARGS(actual.put()))) && actual) {
                VariantClear(&child);
                return actual;
            }
        }
    }
    VariantClear(&child);
    return parent;
}

bool accessibleIsPassword(IAccessible* accessible) {
    if (!accessible) return false;
    VARIANT self;
    VariantInit(&self);
    self.vt = VT_I4;
    self.lVal = CHILDID_SELF;
    VARIANT state;
    VariantInit(&state);
    const HRESULT hr = accessible->get_accState(self, &state);
    const bool protectedField = SUCCEEDED(hr)
        && state.vt == VT_I4
        && (state.lVal & STATE_SYSTEM_PROTECTED) != 0;
    VariantClear(&state);
    return protectedField;
}

bool readUiaWithAncestors(IUIAutomationElement* focused, Result& result, size_t maxChars) {
    if (!focused || !gAutomation) return false;
    if (readTextPattern(focused, result, maxChars, true) && textSize(result) >= kMinimumUsefulChars) return true;

    ComPtr<IUIAutomationTreeWalker> walker;
    if (FAILED(gAutomation->get_ControlViewWalker(walker.put())) || !walker) return textSize(result) > 0;
    focused->AddRef();
    ComPtr<IUIAutomationElement> current(focused);
    for (int depth = 0; depth < kMaxAncestorDepth; ++depth) {
        ComPtr<IUIAutomationElement> parent;
        if (FAILED(walker->GetParentElement(current.get(), parent.put())) || !parent) break;
        current = std::move(parent);
        if (elementIsPassword(current.get())) break;
        readTextPattern(current.get(), result, maxChars, true);
        if (textSize(result) >= kMinimumUsefulChars) break;
    }
    return textSize(result) > 0;
}

HWND focusedWindowForThread(HWND target) {
    DWORD pid = 0;
    DWORD thread = GetWindowThreadProcessId(target, &pid);
    GUITHREADINFO info{sizeof(info)};
    if (thread && GetGUIThreadInfo(thread, &info) && info.hwndFocus) return info.hwndFocus;
    return target;
}

bool readWin32(HWND target, Result& result, size_t maxChars, bool& sensitive) {
    HWND focus = focusedWindowForThread(target);
    wchar_t className[128]{};
    GetClassNameW(focus, className, 128);
    std::wstring classValue(className);
    std::wstring lower = classValue;
    std::transform(lower.begin(), lower.end(), lower.begin(), std::towlower);
    if ((GetWindowLongPtrW(focus, GWL_STYLE) & ES_PASSWORD) != 0) {
        sensitive = true;
        return false;
    }
    if (lower.find(L"edit") == std::wstring::npos && lower.find(L"richedit") == std::wstring::npos) return false;

    DWORD_PTR textLength = 0;
    if (!SendMessageTimeoutW(focus, WM_GETTEXTLENGTH, 0, 0, SMTO_ABORTIFHUNG | SMTO_BLOCK, 80, &textLength)) return false;
    const size_t length = std::min<size_t>(static_cast<size_t>(textLength), maxChars);
    std::wstring text(length + 1, L'\0');
    DWORD_PTR copied = 0;
    if (!SendMessageTimeoutW(focus, WM_GETTEXT, static_cast<WPARAM>(text.size()), reinterpret_cast<LPARAM>(text.data()), SMTO_ABORTIFHUNG | SMTO_BLOCK, 80, &copied)) return false;
    text.resize(std::min<size_t>(static_cast<size_t>(copied), length));
    result.focusedText = trim(text);
    DWORD start = 0, end = 0;
    SendMessageTimeoutW(focus, EM_GETSEL, reinterpret_cast<WPARAM>(&start), reinterpret_cast<LPARAM>(&end), SMTO_ABORTIFHUNG | SMTO_BLOCK, 80, nullptr);
    if (end > start && start < text.size()) {
        result.selectedText = trim(text.substr(start, std::min<size_t>(end - start, text.size() - start)));
    }
    if (!result.focusedText.empty() || !result.selectedText.empty()) result.source = "win32Message";
    return textSize(result) > 0;
}

HRESULT dispatchProperty(IDispatch* object, const wchar_t* name, VARIANT* output) {
    if (!object || !output) return E_POINTER;
    LPOLESTR names[] = {const_cast<LPOLESTR>(name)};
    DISPID id = 0;
    HRESULT hr = object->GetIDsOfNames(IID_NULL, names, 1, LOCALE_USER_DEFAULT, &id);
    if (FAILED(hr)) return hr;
    DISPPARAMS params{};
    VariantInit(output);
    return object->Invoke(id, IID_NULL, LOCALE_USER_DEFAULT, DISPATCH_PROPERTYGET, &params, output, nullptr, nullptr);
}

std::wstring variantText(const VARIANT& value) {
    VARIANT converted;
    VariantInit(&converted);
    if (SUCCEEDED(VariantChangeType(&converted, const_cast<VARIANT*>(&value), 0, VT_BSTR)) && converted.bstrVal) {
        std::wstring result(converted.bstrVal, SysStringLen(converted.bstrVal));
        VariantClear(&converted);
        return trim(result);
    }
    VariantClear(&converted);
    return {};
}

std::wstring dispatchPath(IDispatch* root, std::initializer_list<const wchar_t*> path) {
    if (!root) return {};
    root->AddRef();
    ComPtr<IDispatch> current(root);
    size_t index = 0;
    for (const wchar_t* part : path) {
        ++index;
        VARIANT value;
        HRESULT hr = dispatchProperty(current.get(), part, &value);
        if (FAILED(hr)) return {};
        if (index == path.size()) {
            std::wstring result = variantText(value);
            VariantClear(&value);
            return result;
        }
        IDispatch* next = nullptr;
        if (value.vt == VT_DISPATCH && value.pdispVal) {
            next = value.pdispVal;
            next->AddRef();
        }
        VariantClear(&value);
        if (!next) return {};
        current.reset(next);
    }
    return {};
}

bool readOffice(HWND target, Result& result) {
    ComPtr<IDispatch> native;
    if (FAILED(AccessibleObjectFromWindow(target, static_cast<DWORD>(kObjIdNativeOm), IID_IDispatch, reinterpret_cast<void**>(native.put()))) || !native) return false;
    const std::vector<std::wstring> candidates = {
        dispatchPath(native.get(), {L"Application", L"Selection", L"Text"}),
        dispatchPath(native.get(), {L"Selection", L"Text"}),
        dispatchPath(native.get(), {L"Application", L"ActiveCell", L"Text"}),
        dispatchPath(native.get(), {L"Content", L"Text"}),
        dispatchPath(native.get(), {L"Application", L"ActiveDocument", L"Content", L"Text"}),
    };
    for (const auto& candidate : candidates) {
        if (!candidate.empty()) {
            if (result.focusedText.empty()) result.focusedText = candidate;
            else pushUnique(result.documentText, candidate);
        }
    }
    if (textSize(result) > 0) result.source = "officeNative";
    return textSize(result) > 0;
}

std::wstring accessibleString(IAccessible* accessible, bool value) {
    if (!accessible) return {};
    VARIANT self;
    VariantInit(&self);
    self.vt = VT_I4;
    self.lVal = CHILDID_SELF;
    BSTR text = nullptr;
    HRESULT hr = value ? accessible->get_accValue(self, &text) : accessible->get_accName(self, &text);
    std::wstring output;
    if (SUCCEEDED(hr) && text) output.assign(text, SysStringLen(text));
    if (text) SysFreeString(text);
    return trim(output);
}

bool readMsaaAccessible(IAccessible* accessible, Result& result) {
    result.focusedText = accessibleString(accessible, true);
    if (result.focusedText.empty()) result.focusedText = accessibleString(accessible, false);
    if (!result.focusedText.empty()) result.source = "msaa";
    return !result.focusedText.empty();
}

bool readMsaa(HWND target, Result& result) {
    ComPtr<IAccessible> root;
    if (FAILED(AccessibleObjectFromWindow(target, static_cast<DWORD>(OBJID_CLIENT), IID_IAccessible, reinterpret_cast<void**>(root.put()))) || !root) return false;
    VARIANT focus;
    VariantInit(&focus);
    ComPtr<IAccessible> focused;
    if (SUCCEEDED(root->get_accFocus(&focus)) && focus.vt == VT_DISPATCH && focus.pdispVal) {
        focus.pdispVal->QueryInterface(IID_PPV_ARGS(focused.put()));
    }
    VariantClear(&focus);
    IAccessible* candidate = focused ? focused.get() : root.get();
    return readMsaaAccessible(candidate, result);
}

void sendKey(WORD virtualKey, bool down) {
    INPUT input{};
    input.type = INPUT_KEYBOARD;
    input.ki.wVk = virtualKey;
    if (!down) input.ki.dwFlags = KEYEVENTF_KEYUP;
    SendInput(1, &input, sizeof(input));
}

void sendChord(WORD key) {
    sendKey(VK_CONTROL, true);
    sendKey(key, true);
    sendKey(key, false);
    sendKey(VK_CONTROL, false);
}

std::wstring clipboardUnicodeText() {
    std::wstring output;
    if (!OpenClipboard(nullptr)) return output;
    HANDLE data = GetClipboardData(CF_UNICODETEXT);
    if (data) {
        const wchar_t* text = static_cast<const wchar_t*>(GlobalLock(data));
        if (text) {
            output = text;
            GlobalUnlock(data);
        }
    }
    CloseClipboard();
    return trim(output);
}

bool readClipboardDeep(HWND target, IUIAutomationElement* focused, Result& result) {
    if (GetForegroundWindow() != target) return false;
    CONTROLTYPEID controlType = 0;
    if (!focused || FAILED(focused->get_CurrentControlType(&controlType))) return false;
    if (controlType == UIA_EditControlTypeId || controlType == UIA_ComboBoxControlTypeId) return false;

    ComPtr<IUIAutomationTextRange> originalSelection;
    ComPtr<IUIAutomationTextPattern> selectionPattern;
    if (SUCCEEDED(focused->GetCurrentPatternAs(UIA_TextPatternId, __uuidof(IUIAutomationTextPattern), reinterpret_cast<void**>(selectionPattern.put()))) && selectionPattern) {
        ComPtr<IUIAutomationTextRangeArray> originalRanges;
        if (SUCCEEDED(selectionPattern->GetSelection(originalRanges.put())) && originalRanges) {
            int count = 0;
            if (SUCCEEDED(originalRanges->get_Length(&count)) && count > 0) {
                ComPtr<IUIAutomationTextRange> current;
                if (SUCCEEDED(originalRanges->GetElement(0, current.put())) && current) {
                    current->Clone(originalSelection.put());
                }
            }
        }
    }

    ComPtr<IDataObject> backup;
    OleGetClipboard(backup.put());
    const DWORD before = GetClipboardSequenceNumber();
    sendChord('A');
    Sleep(15);
    sendChord('C');

    std::wstring text;
    DWORD copiedSequence = before;
    for (int attempt = 0; attempt < 25; ++attempt) {
        if (GetForegroundWindow() != target) break;
        if (GetClipboardSequenceNumber() != before) {
            copiedSequence = GetClipboardSequenceNumber();
            text = clipboardUnicodeText();
            if (!text.empty()) break;
        }
        Sleep(10);
    }

    const bool targetStillForeground = GetForegroundWindow() == target;
    if (!targetStillForeground) {
        text.clear();
        result.diagnostics.push_back(L"深度读取期间目标窗口失去焦点，已放弃结果。 ");
    }
    const bool clipboardUnchangedSinceCopy = GetClipboardSequenceNumber() == copiedSequence;
    if (clipboardUnchangedSinceCopy) {
        if (backup) {
            OleSetClipboard(backup.get());
            OleFlushClipboard();
        } else if (OpenClipboard(nullptr)) {
            EmptyClipboard();
            CloseClipboard();
        }
    } else {
        text.clear();
        result.diagnostics.push_back(L"复制期间检测到其他剪贴板修改，已放弃深度读取且未覆盖新内容。 ");
    }

    // 优先利用 UIA 清除 Ctrl+A 产生的临时选择；无法操作时发送 Esc，避免留下整页高亮。
    if (targetStillForeground && selectionPattern) {
        ComPtr<IUIAutomationTextRangeArray> ranges;
        if (SUCCEEDED(selectionPattern->GetSelection(ranges.put())) && ranges) {
            int count = 0;
            if (SUCCEEDED(ranges->get_Length(&count))) {
                for (int index = 0; index < count; ++index) {
                    ComPtr<IUIAutomationTextRange> range;
                    if (SUCCEEDED(ranges->GetElement(index, range.put())) && range) range->RemoveFromSelection();
                }
            }
        }
        if (originalSelection) originalSelection->AddToSelection();
    } else if (targetStillForeground) {
        sendKey(VK_ESCAPE, true);
        sendKey(VK_ESCAPE, false);
    }

    if (text.empty()) return false;
    pushUnique(result.documentText, text);
    result.source = "clipboardDeep";
    return true;
}

bool isBrowserOrElectron(const std::wstring& process) {
    static const std::vector<std::wstring> names = {
        L"chrome.exe", L"msedge.exe", L"firefox.exe", L"code.exe", L"cursor.exe",
        L"discord.exe", L"slack.exe", L"obsidian.exe", L"chatgpt.exe"
    };
    return std::find(names.begin(), names.end(), process) != names.end();
}

bool isOffice(const std::wstring& process) {
    return process == L"winword.exe" || process == L"excel.exe" || process == L"powerpnt.exe";
}

void limitValue(std::wstring& value, size_t& remaining, bool& truncated) {
    value = trim(value);
    if (value.size() > remaining) {
        value.resize(remaining);
        truncated = true;
    }
    remaining -= std::min(remaining, value.size());
}

void limitList(std::vector<std::wstring>& values, size_t& remaining, bool& truncated) {
    std::vector<std::wstring> output;
    for (auto value : values) {
        if (remaining == 0) {
            truncated = true;
            break;
        }
        limitValue(value, remaining, truncated);
        if (!value.empty() && std::find(output.begin(), output.end(), value) == output.end()) output.push_back(std::move(value));
    }
    values = std::move(output);
}

void enforceBudget(Result& result, size_t maxChars) {
    size_t remaining = maxChars;
    limitValue(result.selectedText, remaining, result.truncated);
    limitValue(result.focusedText, remaining, result.truncated);
    limitValue(result.caretContext, remaining, result.truncated);
    limitList(result.visibleText, remaining, result.truncated);
    limitList(result.documentText, remaining, result.truncated);
}

Result capture(const Request& request) {
    const auto started = Clock::now();
    const auto readerDeadline = started + std::chrono::milliseconds(request.readerBudgetMs);
    Result result;
    result.requestId = request.requestId;
    HWND target = reinterpret_cast<HWND>(request.hwnd);
    if (!IsWindow(target) || IsIconic(target)) {
        result.status = "failed";
        result.diagnostics.push_back(L"目标窗口已关闭或最小化。 ");
        return result;
    }
    DWORD actualPid = 0;
    GetWindowThreadProcessId(target, &actualPid);
    if (actualPid != request.pid) {
        result.status = "failed";
        result.diagnostics.push_back(L"目标窗口进程已发生变化。 ");
        return result;
    }
    const std::wstring process = processName(actualPid);
    const std::wstring title = windowTitle(target);
    const bool browser = isBrowserOrElectron(process);
    const bool office = isOffice(process);
    bool sensitive = false;

    // 全局 GetFocusedElement 在 Chromium/Electron 中偶尔会被其多进程辅助功能桥拖慢。
    // 快捷键刚触发时鼠标通常仍位于用户点击的编辑区，先用定点 MSAA/IA2 读取，可绕开全局焦点查询。
    const bool cursorOnTarget = request.hasCursor && pointBelongsToTarget(target, request.cursor);
    ComPtr<IUIAutomationElement> pointed;
    if (cursorOnTarget) {
        const size_t before = textSize(result);
        const auto pointStarted = Clock::now();
        auto accessible = accessibleAtPoint(request.cursor);
        if (accessible && accessibleIsPassword(accessible.get())) {
            sensitive = true;
            result.diagnostics.push_back(L"鼠标所在控件为受保护输入框，已停止读取。 ");
        } else if (accessible) {
            if (browser) readIa2Accessible(accessible.get(), result, request.maxChars, &request.cursor);
            if (textSize(result) == before) readMsaaAccessible(accessible.get(), result);
        }
        const auto pointMs = std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - pointStarted).count();
        std::wostringstream diagnostic;
        diagnostic << L"鼠标定点 MSAA/IA2："
                   << (textSize(result) > before ? L"读取成功" : L"无可用内容")
                   << L"，耗时 " << pointMs << L" ms。 ";
        result.diagnostics.push_back(diagnostic.str());
        if (textSize(result) > before) {
            auto& stats = gReaderStats[process];
            stats.source = result.source.empty() ? "msaa" : result.source;
            const double elapsed = static_cast<double>(pointMs);
            stats.averageMs = stats.samples == 0 ? elapsed : (stats.averageMs * stats.samples + elapsed) / (stats.samples + 1);
            ++stats.samples;
        }
    }
    if (sensitive) {
        result.status = "sensitive";
        return result;
    }
    if (cursorOnTarget && textSize(result) < kMinimumUsefulChars && Clock::now() < readerDeadline) {
        pointed = elementAtPoint(request.cursor, actualPid, process);
        if (pointed && elementIsPassword(pointed.get())) {
            result.status = "sensitive";
            result.diagnostics.push_back(L"鼠标所在 UIA 控件为受保护输入框，已停止读取。 ");
            return result;
        }
        if (pointed) {
            const size_t before = textSize(result);
            const auto pointStarted = Clock::now();
            if (browser) readIa2(pointed.get(), result, request.maxChars);
            if (textSize(result) < kMinimumUsefulChars) readTextPattern(pointed.get(), result, request.maxChars, true);
            const auto pointMs = std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - pointStarted).count();
            std::wostringstream diagnostic;
            diagnostic << L"鼠标定点 UIA："
                       << (textSize(result) > before ? L"读取成功" : L"无可用内容")
                       << L"，耗时 " << pointMs << L" ms。 ";
            result.diagnostics.push_back(diagnostic.str());
        }
    }

    // 定点通道已取得足够正文时，不再调用可能慢的全局焦点 API。
    ComPtr<IUIAutomationElement> focused;
    if (textSize(result) < kMinimumUsefulChars && Clock::now() < readerDeadline) {
        focused = focusedElement(target, actualPid, process);
        if (focused && elementIsPassword(focused.get())) {
            result.status = "sensitive";
            result.diagnostics.push_back(L"焦点位于密码控件，已停止读取。 ");
            return result;
        }
    }
    auto tryReader = [&](const std::string& reader) {
        if (textSize(result) >= kMinimumUsefulChars || sensitive) return;
        if (Clock::now() >= readerDeadline) {
            result.diagnostics.push_back(L"原生读取预算已用尽，跳过后续读取器。 ");
            return;
        }
        if ((reader == "officeNative" && !office) || (reader == "win32Message" && office)) return;
        const size_t before = textSize(result);
        const auto readerStarted = Clock::now();
        if (reader == "officeNative" && office) readOffice(target, result);
        else if (reader == "ia2Text") readIa2(focused.get(), result, request.maxChars);
        else if (reader == "win32Message" && !office) readWin32(target, result, request.maxChars, sensitive);
        else if (reader == "uiaTextPattern") readUiaWithAncestors(focused.get(), result, request.maxChars);
        else if (reader == "msaa") readMsaa(target, result);
        const size_t after = textSize(result);
        const auto elapsedMicros = std::chrono::duration_cast<std::chrono::microseconds>(Clock::now() - readerStarted).count();
        std::wostringstream diagnostic;
        diagnostic << std::wstring(reader.begin(), reader.end()) << L"："
                   << (after > before ? L"读取成功" : L"无可用内容") << L"，耗时 "
                   << static_cast<uint64_t>(elapsedMicros / 1000) << L" ms。 ";
        result.diagnostics.push_back(diagnostic.str());
        if (after > before) {
            result.source = reader;
            const double elapsed = static_cast<double>(elapsedMicros) / 1000.0;
            auto& stats = gReaderStats[process];
            stats.source = reader;
            stats.averageMs = stats.samples == 0 ? elapsed : (stats.averageMs * stats.samples + elapsed) / (stats.samples + 1);
            ++stats.samples;
        }
    };

    if (auto cached = gReaderStats.find(process); cached != gReaderStats.end()) {
        tryReader(cached->second.source);
        if (textSize(result) > 0) {
            std::wostringstream diagnostic;
            diagnostic << L"优先复用应用读取器，历史平均耗时 " << static_cast<uint64_t>(cached->second.averageMs) << L" ms。 ";
            result.diagnostics.push_back(diagnostic.str());
        }
    }
    if (office) tryReader("officeNative");
    if (browser) tryReader("ia2Text");
    if (!browser && !office) tryReader("win32Message");
    if (sensitive) {
        result.status = "sensitive";
        return result;
    }
    tryReader("uiaTextPattern");
    if (!browser) tryReader("ia2Text");
    tryReader("msaa");

    if (result.documentText.empty()) {
        auto cached = gDocumentCache.find(process);
        if (cached != gDocumentCache.end()
            && cached->second.hwnd == target
            && cached->second.title == title
            && cached->second.expires > Clock::now()) {
            result.documentText = cached->second.text;
            result.diagnostics.push_back(L"复用了 2 秒内的限长文档正文缓存。 ");
        }
    }
    if (request.deepClipboard && textSize(result) < kMinimumUsefulChars && Clock::now() < readerDeadline) {
        readClipboardDeep(target, focused ? focused.get() : pointed.get(), result);
    } else if (request.deepClipboard && textSize(result) < kMinimumUsefulChars) {
        result.diagnostics.push_back(L"原生读取预算已用尽，跳过深度剪贴板读取。 ");
    }

    enforceBudget(result, request.maxChars);
    if (!result.documentText.empty()) {
        gDocumentCache[process] = DocumentCache{target, title, result.documentText, Clock::now() + std::chrono::seconds(2)};
    }
    result.status = textSize(result) > 0 ? "captured" : "empty";
    result.elapsedMs = static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - started).count());
    return result;
}

PSECURITY_DESCRIPTOR currentUserPipeSecurity() {
    HANDLE token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) return nullptr;
    DWORD size = 0;
    GetTokenInformation(token, TokenUser, nullptr, 0, &size);
    std::vector<unsigned char> buffer(size);
    if (!GetTokenInformation(token, TokenUser, buffer.data(), size, &size)) {
        CloseHandle(token);
        return nullptr;
    }
    CloseHandle(token);
    auto* user = reinterpret_cast<TOKEN_USER*>(buffer.data());
    LPWSTR sid = nullptr;
    if (!ConvertSidToStringSidW(user->User.Sid, &sid)) return nullptr;
    std::wstring sddl = L"D:P(A;;GA;;;" + std::wstring(sid) + L")";
    LocalFree(sid);
    PSECURITY_DESCRIPTOR descriptor = nullptr;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(), SDDL_REVISION_1, &descriptor, nullptr)) return nullptr;
    return descriptor;
}

std::wstring pipeArgument() {
    int count = 0;
    LPWSTR* arguments = CommandLineToArgvW(GetCommandLineW(), &count);
    std::wstring pipe;
    if (arguments) {
        for (int index = 1; index + 1 < count; ++index) {
            if (std::wstring(arguments[index]) == L"--pipe") pipe = arguments[index + 1];
        }
        LocalFree(arguments);
    }
    return pipe;
}

bool selfTestRequested() {
    return std::wstring(GetCommandLineW()).find(L"--self-test") != std::wstring::npos;
}

int runSelfTests() {
    const auto request = parseRequest(
        R"({"protocolVersion":1,"requestId":42,"hwnd":123,"pid":456,"maxChars":3000,"deepClipboard":true,"cursorX":-320,"cursorY":480})");
    if (!request || request->requestId != 42 || request->hwnd != 123 || request->pid != 456
        || request->maxChars != 3000 || !request->deepClipboard || !request->hasCursor
        || request->cursor.x != -320 || request->cursor.y != 480) return 10;
    if (parseRequest(R"({"protocolVersion":1})")) return 11;

    Result budget;
    budget.requestId = 42;
    budget.selectedText = L"selected";
    budget.focusedText = L"focused";
    budget.documentText = {L"document"};
    enforceBudget(budget, 10);
    if (budget.selectedText != L"selected" || budget.focusedText != L"fo" || !budget.truncated
        || !budget.documentText.empty()) return 12;

    budget.status = "captured";
    const std::string response = serializeResult(budget);
    if (response.find("\"requestId\":42") == std::string::npos
        || response.find("\"status\":\"captured\"") == std::string::npos) return 13;
    return 0;
}

}  // namespace

int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    if (selfTestRequested()) return runSelfTests();
    const std::wstring pipeName = pipeArgument();
    if (pipeName.empty()) return 2;
    if (FAILED(OleInitialize(nullptr))) return 3;
    CoCreateInstance(CLSID_CUIAutomation, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(gAutomation.put()));

    PSECURITY_DESCRIPTOR descriptor = currentUserPipeSecurity();
    if (!descriptor) {
        gAutomation.reset();
        OleUninitialize();
        return 6;
    }
    SECURITY_ATTRIBUTES security{sizeof(security), descriptor, FALSE};
    HANDLE pipe = CreateNamedPipeW(
        pipeName.c_str(),
        PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
        1,
        kPipeBufferBytes,
        kPipeBufferBytes,
        0,
        &security);
    LocalFree(descriptor);
    if (pipe == INVALID_HANDLE_VALUE) {
        OleUninitialize();
        return 4;
    }
    const BOOL connected = ConnectNamedPipe(pipe, nullptr) || GetLastError() == ERROR_PIPE_CONNECTED;
    if (!connected) {
        CloseHandle(pipe);
        OleUninitialize();
        return 5;
    }

    for (;;) {
        uint32_t length = 0;
        if (!readExact(pipe, &length, sizeof(length))) break;
        if (length == 0 || length > kMaxFrameBytes) break;
        std::string requestJson(length, '\0');
        if (!readExact(pipe, requestJson.data(), length)) break;
        auto request = parseRequest(requestJson);
        Result result;
        if (!request || request->protocolVersion != 1) {
            result.requestId = request ? request->requestId : 0;
            result.status = "failed";
            result.diagnostics.push_back(L"文本探针请求格式无效。 ");
        } else {
            try {
                result = capture(*request);
            } catch (...) {
                result.requestId = request->requestId;
                result.status = "failed";
                result.diagnostics.push_back(L"文本探针内部处理异常。 ");
            }
        }
        const std::string response = serializeResult(result);
        const uint32_t responseLength = static_cast<uint32_t>(response.size());
        if (!writeExact(pipe, &responseLength, sizeof(responseLength))
            || !writeExact(pipe, response.data(), responseLength)) break;
        FlushFileBuffers(pipe);
    }

    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    gAutomation.reset();
    OleUninitialize();
    return 0;
}
