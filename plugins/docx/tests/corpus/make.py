"""Builds the test corpus: Word documents written by three producers, so
the reader meets their different ways of writing the same content.

- `python-docx-basic.docx`: python-docx (MIT) over its default template:
  headings, run formatting, lists by style, a table with merged cells, a
  picture, a comment, a page break, a second section, header and footer.
- `handmade-features.docx`: written here part by part, every construct
  the plugin reads: styles with `basedOn` chains and toggle properties, a
  table style with its conditional parts, numbering with levels, overrides
  and restarts, footnotes and endnotes, a comment, tracked changes, simple
  and complex fields, a hyperlink and a bookmark, a content control, a
  text box in `mc:AlternateContent`, a picture, OMML math, symbols, tabs
  and breaks, a section break, header and footer with a PAGE field.
- `libreoffice-*.docx`: each of the two saved again by LibreOffice Writer
  (headless), a second producer's way of writing the same document.

Run from this directory:

    python3 make.py

Needs python-docx and `soffice` on the PATH. The files are generated here
and licensed as the repository is.
"""

import io
import os
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib


def png(width, height, color):
    """A PNG of one color, written without a library."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(color) * width
    raw = row * height
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def basic():
    from docx import Document
    from docx.enum.section import WD_SECTION
    from docx.enum.text import WD_COLOR_INDEX
    from docx.shared import Inches, Pt, RGBColor

    d = Document()
    d.core_properties.title = "Kalem basic"
    d.core_properties.author = "Kalem"
    d.add_heading("Kalem basic", 0)
    d.add_heading("Heading one", 1)
    p = d.add_paragraph("Plain, ")
    p.add_run("bold").bold = True
    p.add_run(", ")
    p.add_run("italic").italic = True
    p.add_run(", ")
    p.add_run("underlined").underline = True
    p.add_run(", ")
    r = p.add_run("red")
    r.font.color.rgb = RGBColor(0xC0, 0x00, 0x00)
    p.add_run(", ")
    r = p.add_run("big")
    r.font.size = Pt(16)
    p.add_run(", ")
    r = p.add_run("mono")
    r.font.name = "Courier New"
    p.add_run(", ")
    r = p.add_run("marked")
    r.font.highlight_color = WD_COLOR_INDEX.YELLOW
    p.add_run(", x")
    r = p.add_run("2")
    r.font.superscript = True
    p.add_run(", ")
    r = p.add_run("struck")
    r.font.strike = True
    p.add_run(", ")
    r = p.add_run("Small Caps")
    r.font.small_caps = True
    p.add_run(".")
    commented = d.add_paragraph().add_run("A commented sentence.")
    d.add_comment(runs=[commented], text="A note on it.", author="Kalem", initials="K")
    d.add_paragraph("Türkçe ğüşıöç İ")
    d.add_paragraph("  spaced  ")
    p = d.add_paragraph("a\tb")
    r = p.add_run()
    r.add_break()
    r.add_text("after a line break")
    d.add_paragraph("First bullet", style="List Bullet")
    d.add_paragraph("Second bullet", style="List Bullet")
    d.add_paragraph("Nested bullet", style="List Bullet 2")
    d.add_paragraph("First number", style="List Number")
    d.add_paragraph("Second number", style="List Number")
    t = d.add_table(rows=3, cols=3)
    t.style = "Table Grid"
    for i, h in enumerate(["Item", "Q1", "Q2"]):
        t.cell(0, i).text = h
    t.cell(1, 0).text = "Rent"
    t.cell(1, 1).merge(t.cell(1, 2)).text = "1,200 both"
    t.cell(2, 0).text = "Food"
    t.cell(2, 1).text = "431.50"
    t.cell(2, 2).text = "512.25"
    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as f:
        f.write(png(16, 16, (200, 30, 30)))
        picture = f.name
    try:
        d.add_picture(picture, width=Inches(0.5))
    finally:
        os.unlink(picture)
    d.add_page_break()
    d.add_heading("Heading two", 2)
    d.add_paragraph("On the second page.")
    d.add_section(WD_SECTION.NEW_PAGE)
    d.add_paragraph("In the second section.")
    d.sections[0].header.paragraphs[0].text = "Basic header"
    d.sections[0].footer.paragraphs[0].text = "Basic footer"
    d.save("python-docx-basic.docx")


W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS = (
    'xmlns:wpc="http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas" '
    'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
    'xmlns:o="urn:schemas-microsoft-com:office:office" '
    f'xmlns:r="{R}" '
    'xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" '
    'xmlns:v="urn:schemas-microsoft-com:vml" '
    'xmlns:wp14="http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    'xmlns:w10="urn:schemas-microsoft-com:office:word" '
    f'xmlns:w="{W}" '
    'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
    'xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml" '
    'xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" '
    'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" '
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" '
    'mc:Ignorable="w14 w15 wp14"'
)
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\r\n'
CT = "application/vnd.openxmlformats-officedocument.wordprocessingml."
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/"


def t(text):
    space = ' xml:space="preserve"' if text != text.strip() else ""
    return f"<w:t{space}>{text}</w:t>"


def run(text, rpr=""):
    rpr = f"<w:rPr>{rpr}</w:rPr>" if rpr else ""
    return f"<w:r>{rpr}{t(text)}</w:r>"


def para(content, ppr=""):
    ppr = f"<w:pPr>{ppr}</w:pPr>" if ppr else ""
    return f"<w:p>{ppr}{content}</w:p>"


def styled(style, text):
    return para(run(text), f'<w:pStyle w:val="{style}"/>')


def listed(num, level, text):
    return para(
        run(text),
        f'<w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="{level}"/>'
        f'<w:numId w:val="{num}"/></w:numPr>',
    )


def field(instr, result):
    return (
        '<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
        f'<w:r><w:instrText xml:space="preserve">{instr}</w:instrText></w:r>'
        '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
        f"{run(result)}"
        '<w:r><w:fldChar w:fldCharType="end"/></w:r>'
    )


def cell(content, width, tcpr=""):
    return f'<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>{tcpr}</w:tcPr>{content}</w:tc>'


TEXT_BOX = (
    "<w:r><mc:AlternateContent><mc:Choice Requires=\"wps\"><w:drawing>"
    '<wp:anchor distT="0" distB="0" distL="114300" distR="114300" simplePos="0" '
    'relativeHeight="251659264" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">'
    '<wp:simplePos x="0" y="0"/>'
    '<wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH>'
    '<wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>'
    '<wp:extent cx="1828800" cy="457200"/><wp:effectExtent l="0" t="0" r="0" b="0"/>'
    '<wp:wrapSquare wrapText="bothSides"/><wp:docPr id="2" name="Text Box 2"/>'
    "<wp:cNvGraphicFramePr/>"
    '<a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
    '<wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr><a:xfrm><a:off x="0" y="0"/>'
    '<a:ext cx="1828800" cy="457200"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>'
    '<a:solidFill><a:schemeClr val="lt1"/></a:solidFill>'
    '<a:ln w="6350"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln></wps:spPr>'
    "<wps:txbx><w:txbxContent>" + para(run("Inside a text box")) + "</w:txbxContent></wps:txbx>"
    '<wps:bodyPr rot="0" vert="horz" wrap="square" lIns="91440" tIns="45720" rIns="91440" '
    'bIns="45720" anchor="t" anchorCtr="0"><a:noAutofit/></wps:bodyPr></wps:wsp>'
    "</a:graphicData></a:graphic></wp:anchor></w:drawing></mc:Choice>"
    "<mc:Fallback><w:pict>"
    '<v:shapetype id="_x0000_t202" coordsize="21600,21600" o:spt="202" '
    'path="m,l,21600r21600,l21600,xe"><v:stroke joinstyle="miter"/>'
    '<v:path gradientshapeok="t" o:connecttype="rect"/></v:shapetype>'
    '<v:shape id="Text Box 2" o:spid="_x0000_s1026" type="#_x0000_t202" '
    'style="position:absolute;margin-left:0;margin-top:0;width:2in;height:36pt;z-index:251659264" '
    'fillcolor="white" strokeweight=".5pt"><v:textbox><w:txbxContent>'
    + para(run("Inside a text box"))
    + "</w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback>"
    "</mc:AlternateContent></w:r>"
)

PICTURE = (
    "<w:r><w:drawing>"
    '<wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="304800" cy="304800"/>'
    '<wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Picture 1" descr="A red square"/>'
    '<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>'
    '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
    '<pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="red.png"/><pic:cNvPicPr/></pic:nvPicPr>'
    '<pic:blipFill><a:blip r:embed="rId20"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
    '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="304800" cy="304800"/></a:xfrm>'
    '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>'
    "</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
)

TRACKED = 'w:author="Ayşe Yılmaz" w:date="2026-10-01T10:00:00Z"'


def features_body():
    b = []
    b.append(styled("Title", "Kalem features"))
    b.append(
        para(
            '<w:bookmarkStart w:id="0" w:name="intro"/>' + run("Introduction") + '<w:bookmarkEnd w:id="0"/>',
            '<w:pStyle w:val="Heading1"/>',
        )
    )
    b.append(
        para(
            run("Plain, ")
            + run("bold", "<w:b/>")
            + run(", ")
            + run("red italic", '<w:i/><w:color w:val="C00000"/>')
            + run(" and a note")
            + '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="1"/></w:r>'
            + run(", an endnote")
            + '<w:r><w:rPr><w:rStyle w:val="EndnoteReference"/></w:rPr><w:endnoteReference w:id="1"/></w:r>'
            + run(".")
        )
    )
    b.append(styled("Heading2", "Styles"))
    # Quote is italic; Emphasis, italic too, toggles it off inside it.
    b.append(
        para(
            run("A quote with ") + run("emphasis", '<w:rStyle w:val="Emphasis"/>') + run(" inside."),
            '<w:pStyle w:val="Quote"/>',
        )
    )
    b.append(
        para(
            run("Theme colored", '<w:color w:val="ED7D31" w:themeColor="accent2"/>')
            + run(", ")
            + run("hidden", "<w:vanish/>")
            + run("shown, ")
            + run("caps", "<w:caps/>")
            + run(", ")
            + run("Cambria", '<w:rFonts w:ascii="Cambria" w:hAnsi="Cambria"/>')
            + run(" and ")
            + run("on green", '<w:highlight w:val="green"/>')
        )
    )
    b.append(styled("Heading2", "Changes"))
    b.append(
        para(
            run("Changed: ")
            + f'<w:ins w:id="10" {TRACKED}>{run("inserted")}</w:ins>'
            + run(" ")
            + f'<w:del w:id="11" {TRACKED}><w:r><w:delText>deleted</w:delText></w:r></w:del>'
            + run(" ")
            + '<w:r><w:rPr><w:b/>'
            + f'<w:rPrChange w:id="12" {TRACKED}><w:rPr/></w:rPrChange></w:rPr>{t("made bold")}</w:r>'
        )
    )
    b.append(
        para(
            '<w:commentRangeStart w:id="0"/>'
            + run("Commented text")
            + '<w:commentRangeEnd w:id="0"/>'
            + '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="0"/></w:r>'
        )
    )
    b.append(styled("Heading2", "Fields and links"))
    b.append(
        para(
            run("Date: ")
            + '<w:fldSimple w:instr=" DATE \\@ &quot;yyyy-MM-dd&quot; ">'
            + run("2026-10-09")
            + "</w:fldSimple>"
            + run("; see ")
            + field(" REF intro \\h ", "Introduction")
            + run("; ")
            + '<w:hyperlink r:id="rId30" w:history="1">'
            + run("Kalem", '<w:rStyle w:val="Hyperlink"/>')
            + "</w:hyperlink>"
            + run(" and ")
            + '<w:hyperlink w:anchor="intro" w:history="1">'
            + run("the start", '<w:rStyle w:val="Hyperlink"/>')
            + "</w:hyperlink>"
            + run(".")
        )
    )
    b.append(styled("Heading2", "Lists"))
    b.append(listed(1, 0, "One"))
    b.append(listed(1, 1, "One point one"))
    b.append(listed(1, 1, "One point two"))
    b.append(listed(1, 2, "Deeper"))
    b.append(listed(1, 0, "Two"))
    b.append(listed(2, 0, "Bullet"))
    b.append(listed(2, 1, "Circle"))
    b.append(listed(2, 2, "Square"))
    b.append(listed(3, 0, "Restarted at one"))
    b.append(listed(4, 0, "Letter a"))
    b.append(listed(4, 1, "Roman i"))
    b.append(listed(4, 1, "Roman ii"))
    b.append(listed(4, 0, "Letter b"))
    b.append(styled("Heading2", "Characters"))
    b.append(
        para(
            run("Tab:")
            + "<w:r><w:tab/>"
            + t("after")
            + "<w:br/>"
            + t("next line")
            + "</w:r>"
            + run(" e")
            + "<w:r><w:noBreakHyphen/></w:r>"
            + run("mail; ")
            + '<w:r><w:sym w:font="Wingdings" w:char="F0FC"/></w:r>'
            + run(" soft")
            + "<w:r><w:softHyphen/></w:r>"
            + run("hyphen")
        )
    )
    b.append(
        para(
            '<w:sdt><w:sdtPr><w:id w:val="1234"/><w14:checkbox><w14:checked w14:val="1"/>'
            '<w14:checkedState w14:val="2612" w14:font="MS Gothic"/>'
            '<w14:uncheckedState w14:val="2610" w14:font="MS Gothic"/></w14:checkbox></w:sdtPr>'
            "<w:sdtContent>"
            + run("☒", '<w:rFonts w:ascii="MS Gothic" w:eastAsia="MS Gothic" w:hAnsi="MS Gothic"/>')
            + "</w:sdtContent></w:sdt>"
            + run(" Done")
        )
    )
    b.append(
        para(
            '<m:oMath><m:r><m:t>x=</m:t></m:r><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num>'
            "<m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"
        )
    )
    b.append(styled("Heading2", "Table"))
    head = "".join(
        cell(para(run(h)), 3000) for h in ["Item", "Q1", "Q2"]
    )
    nested = (
        '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/>'
        '<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" '
        'w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid><w:gridCol w:w="1400"/></w:tblGrid>'
        "<w:tr>" + cell(para(run("nested")), 1400) + "</w:tr></w:tbl>"
    )
    rows = [
        "<w:tr><w:trPr><w:tblHeader/></w:trPr>" + head + "</w:tr>",
        "<w:tr>"
        + cell(para(run("Rent")), 3000)
        + cell(para(run("1,200 both")), 6000, '<w:gridSpan w:val="2"/>')
        + "</w:tr>",
        "<w:tr>"
        + cell(para(run("Food")), 3000, '<w:vMerge w:val="restart"/>')
        + cell(para(run("431.50")), 3000)
        + cell(para(run("512.25")), 3000)
        + "</w:tr>",
        "<w:tr>"
        + cell("<w:p/>", 3000, "<w:vMerge/>")
        + cell(para(run("first")) + para(run("second")), 3000)
        + cell(nested + "<w:p/>", 3000)
        + "</w:tr>",
    ]
    b.append(
        '<w:tbl><w:tblPr><w:tblStyle w:val="KalemTable"/><w:tblW w:w="0" w:type="auto"/>'
        '<w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="1" w:lastColumn="0" '
        'w:noHBand="0" w:noVBand="1"/></w:tblPr>'
        '<w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
        + "".join(rows)
        + "</w:tbl>"
    )
    b.append(styled("Heading2", "Drawings"))
    b.append(para(TEXT_BOX + run("Text beside the box.")))
    b.append(para(PICTURE + run(" A picture.")))
    b.append(para('<w:r><w:br w:type="page"/></w:r>'))
    b.append(
        para(
            run("End of section one."),
            '<w:sectPr><w:type w:val="continuous"/><w:pgSz w:w="11906" w:h="16838"/>'
            '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" '
            'w:footer="708" w:gutter="0"/><w:cols w:space="708"/><w:docGrid w:linePitch="360"/></w:sectPr>',
        )
    )
    b.append(para(run("In section two.")))
    b.append(
        '<w:sectPr><w:headerReference w:type="default" r:id="rId10"/>'
        '<w:footerReference w:type="default" r:id="rId11"/>'
        '<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" '
        'w:left="1440" w:header="708" w:footer="708" w:gutter="0"/><w:cols w:space="708"/>'
        '<w:docGrid w:linePitch="360"/></w:sectPr>'
    )
    return f"{DECL}<w:document {NS}><w:body>{''.join(b)}</w:body></w:document>"


def style(kind, sid, name, body="", default=False, based=None, nxt=None):
    d = ' w:default="1"' if default else ""
    based = f'<w:basedOn w:val="{based}"/>' if based else ""
    nxt = f'<w:next w:val="{nxt}"/>' if nxt else ""
    return f'<w:style w:type="{kind}"{d} w:styleId="{sid}"><w:name w:val="{name}"/>{based}{nxt}{body}</w:style>'


def styles():
    s = [
        "<w:docDefaults><w:rPrDefault><w:rPr>"
        '<w:rFonts w:asciiTheme="minorHAnsi" w:eastAsiaTheme="minorEastAsia" w:hAnsiTheme="minorHAnsi" w:cstheme="minorBidi"/>'
        '<w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/>'
        "</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>"
        '<w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>',
        style("paragraph", "Normal", "Normal", "<w:qFormat/>", default=True),
        style("character", "DefaultParagraphFont", "Default Paragraph Font",
              '<w:uiPriority w:val="1"/><w:semiHidden/><w:unhideWhenUsed/>', default=True),
        style("table", "TableNormal", "Normal Table",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/>'
              '<w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/>'
              '<w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr>',
              default=True),
        style("numbering", "NoList", "No List", '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/>',
              default=True),
        style("paragraph", "Heading1", "heading 1",
              '<w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/>'
              '<w:spacing w:before="240" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr>'
              '<w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:hAnsiTheme="majorHAnsi" w:cstheme="majorBidi"/>'
              '<w:color w:val="2F5496" w:themeColor="accent1" w:themeShade="BF"/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>',
              based="Normal", nxt="Normal"),
        # Based on Heading 1, not Normal: a chain of three.
        style("paragraph", "Heading2", "heading 2",
              '<w:uiPriority w:val="9"/><w:unhideWhenUsed/><w:qFormat/><w:pPr><w:spacing w:before="40"/>'
              '<w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr>',
              based="Heading1", nxt="Normal"),
        style("paragraph", "Title", "Title",
              '<w:uiPriority w:val="10"/><w:qFormat/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/>'
              '<w:contextualSpacing/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" '
              'w:hAnsiTheme="majorHAnsi" w:cstheme="majorBidi"/><w:spacing w:val="-10"/><w:kern w:val="28"/>'
              '<w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr>',
              based="Normal", nxt="Normal"),
        style("paragraph", "Quote", "Quote",
              '<w:uiPriority w:val="29"/><w:qFormat/><w:pPr><w:spacing w:before="200"/><w:ind w:left="864" w:right="864"/>'
              '<w:jc w:val="center"/></w:pPr><w:rPr><w:i/><w:iCs/><w:color w:val="404040" w:themeColor="text1" '
              'w:themeTint="BF"/></w:rPr>',
              based="Normal", nxt="Normal"),
        style("character", "Emphasis", "Emphasis",
              '<w:uiPriority w:val="20"/><w:qFormat/><w:rPr><w:i/><w:iCs/></w:rPr>', based="DefaultParagraphFont"),
        style("character", "Strong", "Strong",
              '<w:uiPriority w:val="22"/><w:qFormat/><w:rPr><w:b/><w:bCs/></w:rPr>', based="DefaultParagraphFont"),
        style("character", "Hyperlink", "Hyperlink",
              '<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:rPr><w:color w:val="0563C1" w:themeColor="hyperlink"/>'
              '<w:u w:val="single"/></w:rPr>', based="DefaultParagraphFont"),
        style("paragraph", "ListParagraph", "List Paragraph",
              '<w:uiPriority w:val="34"/><w:qFormat/><w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr>',
              based="Normal"),
        style("paragraph", "FootnoteText", "footnote text",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:spacing w:after="0" '
              'w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>',
              based="Normal"),
        style("character", "FootnoteReference", "footnote reference",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>',
              based="DefaultParagraphFont"),
        style("paragraph", "EndnoteText", "endnote text",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:pPr><w:spacing w:after="0" '
              'w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>',
              based="Normal"),
        style("character", "EndnoteReference", "endnote reference",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>',
              based="DefaultParagraphFont"),
        style("character", "CommentReference", "annotation reference",
              '<w:uiPriority w:val="99"/><w:semiHidden/><w:unhideWhenUsed/><w:rPr><w:sz w:val="16"/><w:szCs w:val="16"/></w:rPr>',
              based="DefaultParagraphFont"),
        style("paragraph", "CommentText", "annotation text",
              '<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:spacing w:line="240" w:lineRule="auto"/></w:pPr>'
              '<w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>', based="Normal"),
        style("paragraph", "Header", "header",
              '<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4513"/>'
              '<w:tab w:val="right" w:pos="9026"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>',
              based="Normal"),
        style("paragraph", "Footer", "footer",
              '<w:uiPriority w:val="99"/><w:unhideWhenUsed/><w:pPr><w:tabs><w:tab w:val="center" w:pos="4513"/>'
              '<w:tab w:val="right" w:pos="9026"/></w:tabs><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>',
              based="Normal"),
        style("table", "TableGrid", "Table Grid",
              '<w:uiPriority w:val="39"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>'
              '<w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/>'
              '<w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>'
              '<w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/>'
              '<w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/>'
              '<w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/>'
              '<w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/></w:tblBorders></w:tblPr>',
              based="TableNormal"),
        # A table style of its own, with a header row and banded rows.
        style("table", "KalemTable", "Kalem Table",
              '<w:uiPriority w:val="40"/><w:tblPr><w:tblStyleRowBandSize w:val="1"/></w:tblPr>'
              '<w:tblStylePr w:type="firstRow"><w:rPr><w:b/><w:bCs/><w:color w:val="FFFFFF" w:themeColor="background1"/></w:rPr>'
              '<w:tblPr/><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="4472C4" w:themeFill="accent1"/></w:tcPr></w:tblStylePr>'
              '<w:tblStylePr w:type="firstCol"><w:rPr><w:i/></w:rPr></w:tblStylePr>'
              '<w:tblStylePr w:type="band1Horz"><w:tblPr/><w:tcPr><w:shd w:val="clear" w:color="auto" w:fill="D9E2F3" '
              'w:themeFill="accent1" w:themeFillTint="33"/></w:tcPr></w:tblStylePr>',
              based="TableGrid"),
    ]
    return (
        f'{DECL}<w:styles xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
        f'xmlns:r="{R}" xmlns:w="{W}" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" '
        f'mc:Ignorable="w14">{"".join(s)}</w:styles>'
    )


def level(i, fmt, text, indent, font=None, restart=None):
    rpr = f'<w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:hint="default"/></w:rPr>' if font else ""
    restart = f'<w:lvlRestart w:val="{restart}"/>' if restart is not None else ""
    return (
        f'<w:lvl w:ilvl="{i}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/>{restart}<w:lvlText w:val="{text}"/>'
        f'<w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{indent}" w:hanging="360"/></w:pPr>{rpr}</w:lvl>'
    )


def numbering():
    decimal = "".join(
        level(i, "decimal", ".".join(f"%{j + 1}" for j in range(i + 1)) + ".", 360 * (i + 1) + 360) for i in range(3)
    )
    bullets = (
        level(0, "bullet", "\uf0b7", 720, "Symbol")
        + level(1, "bullet", "o", 1440, "Courier New")
        + level(2, "bullet", "\uf0a7", 2160, "Wingdings")
    )
    letters = level(0, "lowerLetter", "%1)", 720) + level(1, "lowerRoman", "%2.", 1440)
    return (
        f'{DECL}<w:numbering xmlns:w="{W}">'
        f'<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="multilevel"/>{decimal}</w:abstractNum>'
        f'<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>{bullets}</w:abstractNum>'
        f'<w:abstractNum w:abstractNumId="2"><w:multiLevelType w:val="hybridMultilevel"/>{letters}</w:abstractNum>'
        '<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>'
        '<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>'
        '<w:num w:numId="3"><w:abstractNumId w:val="0"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/>'
        "</w:lvlOverride></w:num>"
        '<w:num w:numId="4"><w:abstractNumId w:val="2"/></w:num>'
        "</w:numbering>"
    )


SETTINGS = (
    f'{DECL}<w:settings xmlns:w="{W}"><w:zoom w:percent="100"/><w:defaultTabStop w:val="720"/>'
    '<w:characterSpacingControl w:val="doNotCompress"/>'
    '<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr>'
    '<w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr>'
    '<w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" '
    'w:val="15"/></w:compat><w:themeFontLang w:val="en-US"/>'
    '<w:clrSchemeMapping w:bg1="light1" w:t1="dark1" w:bg2="light2" w:t2="dark2" w:accent1="accent1" '
    'w:accent2="accent2" w:accent3="accent3" w:accent4="accent4" w:accent5="accent5" w:accent6="accent6" '
    'w:hyperlink="hyperlink" w:followedHyperlink="followedHyperlink"/>'
    '<w:decimalSymbol w:val="."/><w:listSeparator w:val=","/></w:settings>'
)


def notes(kind):
    tag, ref, sty = ("footnote", "footnoteRef", "Footnote") if kind == "foot" else ("endnote", "endnoteRef", "Endnote")
    sep = '<w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>'
    text = "A footnote, with " if kind == "foot" else "An endnote, with "
    return (
        f'{DECL}<w:{tag}s {NS}>'
        f'<w:{tag} w:type="separator" w:id="-1"><w:p>{sep}<w:r><w:separator/></w:r></w:p></w:{tag}>'
        f'<w:{tag} w:type="continuationSeparator" w:id="0"><w:p>{sep}<w:r><w:continuationSeparator/></w:r></w:p></w:{tag}>'
        f'<w:{tag} w:id="1"><w:p><w:pPr><w:pStyle w:val="{sty}Text"/></w:pPr>'
        f'<w:r><w:rPr><w:rStyle w:val="{sty}Reference"/></w:rPr><w:{ref}/></w:r>'
        f'{run(" " + text)}{run("italics", "<w:i/>")}{run(".")}</w:p></w:{tag}></w:{tag}s>'
    )


COMMENTS = (
    f'{DECL}<w:comments {NS}><w:comment w:id="0" w:author="Ayşe Yılmaz" w:date="2026-10-01T09:00:00Z" '
    'w:initials="AY"><w:p><w:pPr><w:pStyle w:val="CommentText"/></w:pPr>'
    '<w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r>'
    f'{run("Check this.")}</w:p></w:comment></w:comments>'
)

HEADER = f'{DECL}<w:hdr {NS}>{styled("Header", "Kalem test header")}</w:hdr>'
FOOTER = (
    f'{DECL}<w:ftr {NS}><w:p><w:pPr><w:pStyle w:val="Footer"/><w:jc w:val="center"/></w:pPr>'
    f'{run("Page ")}{field(" PAGE ", "1")}</w:p></w:ftr>'
)

FILL = '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
THEME = (
    f'{DECL}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme">'
    '<a:themeElements><a:clrScheme name="Office">'
    '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>'
    '<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>'
    '<a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>'
    '<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>'
    '<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>'
    '<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>'
    '</a:clrScheme><a:fontScheme name="Office">'
    '<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
    '</a:fontScheme><a:fmtScheme name="Office">'
    f"<a:fillStyleLst>{FILL * 3}</a:fillStyleLst>"
    '<a:lnStyleLst>' + "".join(f'<a:ln w="{w}">{FILL}</a:ln>' for w in (6350, 12700, 19050)) + "</a:lnStyleLst>"
    "<a:effectStyleLst>" + "<a:effectStyle><a:effectLst/></a:effectStyle>" * 3 + "</a:effectStyleLst>"
    f"<a:bgFillStyleLst>{FILL * 3}</a:bgFillStyleLst>"
    "</a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"
)

CORE = (
    f'{DECL}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" '
    'xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">'
    "<dc:title>Kalem features</dc:title><dc:creator>Kalem</dc:creator><cp:lastModifiedBy>Kalem</cp:lastModifiedBy>"
    '<cp:revision>1</cp:revision><dcterms:created xsi:type="dcterms:W3CDTF">2026-10-09T00:00:00Z</dcterms:created>'
    '<dcterms:modified xsi:type="dcterms:W3CDTF">2026-10-09T00:00:00Z</dcterms:modified></cp:coreProperties>'
)
APP = (
    f'{DECL}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" '
    'xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">'
    "<Application>Kalem corpus</Application><Pages>2</Pages><Words>100</Words></Properties>"
)


def features():
    types = (
        f'{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Default Extension="png" ContentType="image/png"/>'
        f'<Override PartName="/word/document.xml" ContentType="{CT}document.main+xml"/>'
        f'<Override PartName="/word/styles.xml" ContentType="{CT}styles+xml"/>'
        f'<Override PartName="/word/numbering.xml" ContentType="{CT}numbering+xml"/>'
        f'<Override PartName="/word/settings.xml" ContentType="{CT}settings+xml"/>'
        f'<Override PartName="/word/footnotes.xml" ContentType="{CT}footnotes+xml"/>'
        f'<Override PartName="/word/endnotes.xml" ContentType="{CT}endnotes+xml"/>'
        f'<Override PartName="/word/comments.xml" ContentType="{CT}comments+xml"/>'
        f'<Override PartName="/word/header1.xml" ContentType="{CT}header+xml"/>'
        f'<Override PartName="/word/footer1.xml" ContentType="{CT}footer+xml"/>'
        '<Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>'
        '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
        '<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>'
        "</Types>"
    )
    root = (
        f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        f'<Relationship Id="rId1" Type="{REL}officeDocument" Target="word/document.xml"/>'
        '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
        f'<Relationship Id="rId3" Type="{REL}extended-properties" Target="docProps/app.xml"/>'
        "</Relationships>"
    )
    rels = [
        ("rId1", "styles", "styles.xml"),
        ("rId2", "numbering", "numbering.xml"),
        ("rId3", "settings", "settings.xml"),
        ("rId4", "footnotes", "footnotes.xml"),
        ("rId5", "endnotes", "endnotes.xml"),
        ("rId6", "comments", "comments.xml"),
        ("rId7", "theme", "theme/theme1.xml"),
        ("rId10", "header", "header1.xml"),
        ("rId11", "footer", "footer1.xml"),
        ("rId20", "image", "media/image1.png"),
    ]
    doc_rels = (
        f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + "".join(f'<Relationship Id="{i}" Type="{REL}{k}" Target="{tg}"/>' for i, k, tg in rels)
        + f'<Relationship Id="rId30" Type="{REL}hyperlink" Target="https://github.com/getkalem/kalem" TargetMode="External"/>'
        + "</Relationships>"
    )
    parts = [
        ("[Content_Types].xml", types),
        ("_rels/.rels", root),
        ("docProps/core.xml", CORE),
        ("docProps/app.xml", APP),
        ("word/document.xml", features_body()),
        ("word/_rels/document.xml.rels", doc_rels),
        ("word/styles.xml", styles()),
        ("word/numbering.xml", numbering()),
        ("word/settings.xml", SETTINGS),
        ("word/footnotes.xml", notes("foot")),
        ("word/endnotes.xml", notes("end")),
        ("word/comments.xml", COMMENTS),
        ("word/header1.xml", HEADER),
        ("word/footer1.xml", FOOTER),
        ("word/theme/theme1.xml", THEME),
    ]
    with zipfile.ZipFile("handmade-features.docx", "w", zipfile.ZIP_DEFLATED) as z:
        for name, text in parts:
            # A fixed time, so that the file is the same on every run.
            info = zipfile.ZipInfo(name, (2026, 10, 9, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, text.encode("utf-8"))
        info = zipfile.ZipInfo("word/media/image1.png", (2026, 10, 9, 0, 0, 0))
        z.writestr(info, png(16, 16, (200, 30, 30)))


def via_libreoffice(src, dst):
    with tempfile.TemporaryDirectory() as out:
        subprocess.run(
            ["soffice", "--headless", "--convert-to", "docx:MS Word 2007 XML", "--outdir", out, src],
            check=True,
            capture_output=True,
        )
        os.replace(os.path.join(out, os.path.splitext(os.path.basename(src))[0] + ".docx"), dst)


if __name__ == "__main__":
    basic()
    features()
    via_libreoffice("python-docx-basic.docx", "libreoffice-basic.docx")
    via_libreoffice("handmade-features.docx", "libreoffice-features.docx")
    print("written:", *sorted(f for f in os.listdir(".") if f.endswith(".docx")), file=sys.stderr)
