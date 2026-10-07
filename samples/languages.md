---
title: nevis language test
covers: right-to-left, mixed direction, complex shaping, CJK, emoji
---

# Language test — تست زبان‌ها

Each line should sit on its own side: English on the left, Persian, Arabic, Hebrew and Urdu on the right.
Press **Ctrl+E** to compare with the source, and again to come back.

## English

The quick brown fox jumps over the lazy dog. **Bold**, *italic*, ~~struck~~, `code` and [a link](https://example.com).

## فارسی

این یک متن آزمایشی به زبان فارسی است. نیم‌فاصله باید درست دیده شود: می‌خواهم، کتاب‌ها، خانه‌ای، پیش‌نمایش.
اعداد فارسی: ۰۱۲۳۴۵۶۷۸۹ — و عدد لاتین 2026 وسط متن.
یک **کلمهٔ پررنگ**، یک *کلمهٔ کج* و یک `کد` در یک خط.

### فهرست کارها

- [ ] خرید نان و شیر
- [x] نوشتن یادداشت
- موردی با یک English word وسطش
  - زیرمورد فارسی
  - nested English item

> الا یا ایها الساقی ادر کأسا و ناولها
> که عشق آسان نمود اول ولی افتاد مشکل‌ها
> — حافظ

> [!NOTE]
> این یک callout فارسی است.

یک پاراگراف بلند برای آزمایش شکستن خط: وقتی پنجره را باریک‌تر کنید، این جمله باید از راست به چپ و بدون به‌هم‌ریختگی به خط بعد برود، و کلمه‌های English و اعداد 42 و ۴۲ باید سر جای خودشان بمانند.

## العربية

مرحبا بالعالم. هذا نص تجريبي باللغة العربية.
مع التشكيل: اللُّغَةُ العَرَبِيَّةُ جَمِيلَةٌ.
الأرقام العربية: ٠١٢٣٤٥٦٧٨٩

## עברית

שלום עולם. זהו טקסט לבדיקה בעברית, עם המספר 123 ומילה באנגלית: nevis.
עם ניקוד: שָׁלוֹם.

## اردو

یہ اردو زبان میں ایک آزمائشی جملہ ہے۔

## Mixed direction

- An English sentence with a فارسی word in the middle.
- جملهٔ فارسی با یک کلمهٔ English وسطش.
- قیمت 1,250,000 تومان است، یا با ارقام فارسی ۱٬۲۵۰٬۰۰۰ تومان.
- (این متن داخل پرانتز است) and (this one is English).
- آیا علامت سؤال در جای درست است؟
- فایل `~/Projects/nevis/README.md` را باز کن.
- سایت https://gitlab.com را ببینید.
- הטקסט הזה mixes עברית and English.

## European languages

**Italiano:** Nel mezzo del cammin di nostra vita / mi ritrovai per una selva oscura. Perché, città, caffè, così.

**Deutsch:** Victor jagt zwölf Boxkämpfer quer über den großen Sylter Deich. „Anführungszeichen“.

**Français :** Portez ce vieux whisky au juge blond qui fume. « Guillemets », cœur, œuvre.

**Español:** El veloz murciélago hindú comía feliz cardillo y kiwi. ¿Dónde? ¡Aquí!

**Polski:** Zażółć gęślą jaźń.

**Türkçe:** Pijamalı hasta yağız şoföre çabucak güvendi. İ ı Ğ ğ Ş ş.

**Tiếng Việt:** Xin chào thế giới. Dấu chồng: ấ ầ ẩ ẫ ậ ơ ư đ.

## Cyrillic and Greek

**Русский:** Съешь же ещё этих мягких французских булок, да выпей чаю.

**Ελληνικά:** Ξεσκεπάζω την ψυχοφθόρα βδελυγμία.

## Other scripts

**हिन्दी:** नमस्ते दुनिया। यह एक परीक्षण वाक्य है।

**ไทย:** สวัสดีชาวโลก ภาษาไทยไม่มีการเว้นวรรคระหว่างคำ

**ქართული:** გამარჯობა მსოფლიო

**Հայերեն:** Բարեւ աշխարհ

## CJK

**中文：** 你好，世界！这是一个测试句子。繁體：資訊、軟體。

**日本語：** こんにちは世界。これはテストの文です。ひらがな、カタカナ、漢字。

**한국어:** 안녕하세요, 세계! 이것은 테스트 문장입니다.

## Emoji and symbols

👋 🌍 🚀 ✅ ❤️ 👨‍👩‍👧 🇮🇷 🇮🇹 🏳️‍🌈

Math: ∑ ∫ √ ∞ ≠ ≤ ≥ → ⇒ ∀ ∃ ∈
Currency: € £ ¥ ₹ ﷼ (Rial)
Other: © ® ™ ½ ¾

## Table

| Language | Hello | Direction |
|----------|-------|:---------:|
| English | Hello | LTR |
| فارسی | سلام | RTL |
| العربية | مرحبا | RTL |
| עברית | שלום | RTL |
| Русский | Привет | LTR |
| 中文 | 你好 | LTR |
| 日本語 | こんにちは | LTR |
| 한국어 | 안녕하세요 | LTR |
| हिन्दी | नमस्ते | LTR |
| ไทย | สวัสดี | LTR |

## Code

```python
greetings = {"fa": "سلام", "ar": "مرحبا", "he": "שלום", "zh": "你好", "hi": "नमस्ते"}
for lang, word in greetings.items():
    print(f"{lang}: {word}")
```

---

Links: [[یادداشت دیگر]] · [[Another Note]] · [Persian Wikipedia](https://fa.wikipedia.org)
