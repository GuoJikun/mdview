//! 文本编码：把磁盘字节解成 UTF-8 字符串，以及按原编码写回去。
//!
//! 内核内存态**永远**是 UTF-8 的 `String`——字节偏移、停靠点、光标全按 UTF-8
//! 字符算。非 UTF-8 只在进出磁盘的那一层出现：读入时转成 UTF-8，保存时按读入
//! 时的编码转回去。这样既不用让整个内核认识编码，也不会把用户的 GBK 文件悄悄
//! 改成 UTF-8（那会让 diff、版本历史、其它编辑器里的一切都变一遍）。
//!
//! GBK 系旧文件在 Windows 上非常常见（记事本存盘、老 OA 导出的 .txt），
//! 只按 UTF-8 读会把整篇变成替换字符。

use encoding_rs::{GB18030, UTF_8};

/// 一份文本的磁盘编码。`Utf8Bom` 单独列出来是因为「有 BOM」与「没 BOM」在
/// 回写时是两个不同的结果，不能合并成「UTF-8」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// 无 BOM 的 UTF-8。绝大多数 Markdown。
    Utf8,
    /// 带 BOM 的 UTF-8（Windows 记事本、部分导出工具）。
    Utf8Bom,
    /// GB18030。GBK / GB2312 是它的子集，用同一套码表解码即可。
    Gb18030,
}

impl TextEncoding {
    /// 给前端与日志用的短标识（也是 `Snapshot.encoding` 的值）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf8Bom => "utf-8-bom",
            Self::Gb18030 => "gb18030",
        }
    }

    /// 状态栏里显示的写法。
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Gb18030 => "GB18030",
        }
    }

    /// 是否需要提醒用户「这个文件不是 UTF-8」。
    pub fn is_utf8(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom)
    }
}

/// UTF-8 BOM 的三个字节。
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// 把磁盘字节解成文本，并报告判定出的编码。
///
/// 顺序是「先严格 UTF-8，再退 GB18030」：
/// - 有 BOM → 一定是 UTF-8（BOM 是它的自证），剥掉 BOM 后按 UTF-8 解；
/// - 否则 `str::from_utf8` 通过就是 UTF-8。**严格**校验而不是 lossy：一段
///   合法的 ASCII 在 GBK 里也是同一个意思，所以先判 UTF-8 不会误伤；
/// - 校验失败说明不是 UTF-8，按 GB18030 解。GB18030 的编码空间覆盖全部
///   Unicode，几乎不会失败；实在解不了的字节会变成 U+FFFD，但那是坏文件，
///   不是我们的判定问题。
pub fn decode(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some(rest) = bytes.strip_prefix(&BOM) {
        let (text, _) = UTF_8.decode_without_bom_handling(rest);
        return (text.into_owned(), TextEncoding::Utf8Bom);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), TextEncoding::Utf8),
        Err(_) => {
            let (text, _) = GB18030.decode_without_bom_handling(bytes);
            (text.into_owned(), TextEncoding::Gb18030)
        }
    }
}

/// 只判定编码，不做解码。
pub fn detect(bytes: &[u8]) -> TextEncoding {
    decode(bytes).1
}

/// 按指定编码把文本变回磁盘字节。
///
/// GB18030 编码不了的字符（例如某些 emoji 之外的生僻符号）会被写成数字字符
/// 引用 `&#...;` ——这是 `encoding_rs` 的既定行为。宁可显示成一串引用，也比
/// 悄悄丢字或整篇保存失败好。
pub fn encode(text: &str, encoding: TextEncoding) -> Vec<u8> {
    match encoding {
        TextEncoding::Utf8 => text.as_bytes().to_vec(),
        TextEncoding::Utf8Bom => {
            let mut out = Vec::with_capacity(text.len() + BOM.len());
            out.extend_from_slice(&BOM);
            out.extend_from_slice(text.as_bytes());
            out
        }
        TextEncoding::Gb18030 => {
            let (bytes, _, _) = GB18030.encode(text);
            bytes.into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「中文测试」的 GB18030 编码（GBK 子集，与记事本存出来的一致）。
    const ZH_GBK: [u8; 8] = [0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4];

    #[test]
    fn plain_utf8_is_decoded_as_utf8_without_bom() {
        let (text, encoding) = decode("中文".as_bytes());
        assert_eq!(text, "中文");
        assert_eq!(encoding, TextEncoding::Utf8);
        assert_eq!(encode(&text, encoding), "中文".as_bytes());
    }

    #[test]
    fn bom_is_stripped_and_remembered() {
        let mut bytes = BOM.to_vec();
        bytes.extend_from_slice("中文".as_bytes());
        let (text, encoding) = decode(&bytes);
        assert_eq!(text, "中文", "BOM 不能留在正文里（否则首个标题识别不出来）");
        assert_eq!(encoding, TextEncoding::Utf8Bom);
        assert_eq!(encode(&text, encoding), bytes, "回写要原样带 BOM");
    }

    #[test]
    fn gbk_bytes_fall_back_to_gb18030() {
        let (text, encoding) = decode(&ZH_GBK);
        assert_eq!(text, "中文测试");
        assert_eq!(encoding, TextEncoding::Gb18030);
        assert_eq!(encode(&text, encoding), ZH_GBK, "回写要保持原编码的字节");
    }

    #[test]
    fn ascii_is_utf8_and_identical_either_way() {
        let (text, encoding) = decode(b"# Title\n");
        assert_eq!(text, "# Title\n");
        assert_eq!(encoding, TextEncoding::Utf8);
    }

    #[test]
    fn mixed_document_round_trips_through_gb18030() {
        let source = "# 标题\r\n\r\n这段是正文，含标点：、。「」\r\n";
        let bytes = encode(source, TextEncoding::Gb18030);
        let (text, encoding) = decode(&bytes);
        assert_eq!(encoding, TextEncoding::Gb18030);
        assert_eq!(text, source, "换行与标点都要原样回来");
    }

    #[test]
    fn detect_does_not_need_to_decode() {
        assert_eq!(detect(b"abc"), TextEncoding::Utf8);
        assert_eq!(detect(&ZH_GBK), TextEncoding::Gb18030);
        let mut bom = BOM.to_vec();
        bom.extend_from_slice(b"x");
        assert_eq!(detect(&bom), TextEncoding::Utf8Bom);
    }

    #[test]
    fn labels_and_kind_match_the_wire_values() {
        assert_eq!(TextEncoding::Utf8.as_str(), "utf-8");
        assert_eq!(TextEncoding::Utf8Bom.as_str(), "utf-8-bom");
        assert_eq!(TextEncoding::Gb18030.as_str(), "gb18030");
        assert_eq!(TextEncoding::Gb18030.label(), "GB18030");
        assert!(!TextEncoding::Gb18030.is_utf8());
        assert!(TextEncoding::Utf8Bom.is_utf8());
    }

    #[test]
    fn empty_text_round_trips_only_loses_the_gbk_tag() {
        // 空文件的字节序列是空的，严格 UTF-8 校验一定通过——所以「空 + GB18030」
        // 会被重新判成 UTF-8。这没有坏处（空文件不区分编码），但要写下来。
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Gb18030,
        ] {
            let (text, detected) = decode(&encode("", encoding));
            assert_eq!(text, "");
            if encoding == TextEncoding::Gb18030 {
                assert_eq!(detected, TextEncoding::Utf8);
            } else {
                assert_eq!(detected, encoding, "空文件只有 BOM 时也要认出自己的编码");
            }
        }
    }
}
