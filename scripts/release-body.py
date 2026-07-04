import xml.etree.ElementTree as ET
import sys

tree = ET.parse(sys.argv[1])
release = tree.find(".//releases/release")
if release is None:
    sys.exit(0)

desc = release.find("description")
if desc is None:
    sys.exit(0)

lines = []
for child in desc:
    if child.tag == "p":
        lines.append(child.text.strip() if child.text else "")
    elif child.tag == "ul":
        for li in child:
            text = "".join(li.itertext()).strip()
            if text:
                lines.append(f"- {text}")

print("\n".join(lines))
