#pragma once

#include <Unknwn.h>
#include <oaidl.h>

// IAccessible2 1.3 的只读文本接口。这里只声明探针需要使用的 ABI 前缀；
// 方法顺序和 UUID 来自 Linux Foundation IAccessible2 的 ia2_api_all.idl。
enum IA2CoordinateType : long {
    IA2_COORDTYPE_SCREEN_RELATIVE = 0,
    IA2_COORDTYPE_PARENT_RELATIVE = 1,
};

enum IA2TextBoundaryType : long {
    IA2_TEXT_BOUNDARY_CHAR = 0,
    IA2_TEXT_BOUNDARY_WORD = 1,
    IA2_TEXT_BOUNDARY_SENTENCE = 2,
    IA2_TEXT_BOUNDARY_PARAGRAPH = 3,
    IA2_TEXT_BOUNDARY_LINE = 4,
    IA2_TEXT_BOUNDARY_ALL = 5,
};

enum IA2ScrollType : long {
    IA2_SCROLL_TYPE_TOP_LEFT = 0,
};

MIDL_INTERFACE("24FD2FFB-3AAD-4A08-8335-A3AD89C0FB4B")
IAccessibleText : public IUnknown {
public:
    virtual HRESULT STDMETHODCALLTYPE addSelection(long startOffset, long endOffset) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_attributes(long offset, long* startOffset, long* endOffset, BSTR* textAttributes) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_caretOffset(long* offset) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_characterExtents(long offset, IA2CoordinateType coordType, long* x, long* y, long* width, long* height) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_nSelections(long* nSelections) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_offsetAtPoint(long x, long y, IA2CoordinateType coordType, long* offset) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_selection(long selectionIndex, long* startOffset, long* endOffset) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_text(long startOffset, long endOffset, BSTR* text) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_textBeforeOffset(long offset, IA2TextBoundaryType boundaryType, long* startOffset, long* endOffset, BSTR* text) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_textAfterOffset(long offset, IA2TextBoundaryType boundaryType, long* startOffset, long* endOffset, BSTR* text) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_textAtOffset(long offset, IA2TextBoundaryType boundaryType, long* startOffset, long* endOffset, BSTR* text) = 0;
    virtual HRESULT STDMETHODCALLTYPE removeSelection(long selectionIndex) = 0;
    virtual HRESULT STDMETHODCALLTYPE setCaretOffset(long offset) = 0;
    virtual HRESULT STDMETHODCALLTYPE setSelection(long selectionIndex, long startOffset, long endOffset) = 0;
    virtual HRESULT STDMETHODCALLTYPE get_nCharacters(long* nCharacters) = 0;
    virtual HRESULT STDMETHODCALLTYPE scrollSubstringTo(long startIndex, long endIndex, IA2ScrollType scrollType) = 0;
    virtual HRESULT STDMETHODCALLTYPE scrollSubstringToPoint(long startIndex, long endIndex, IA2CoordinateType coordinateType, long x, long y) = 0;
};
