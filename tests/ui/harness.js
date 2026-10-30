// Shared QML/JS helpers for the page-level suites (tst_*.qml siblings).
// A .pragma library keeps these functions stateless and shared between
// suite files without polluting object scope.
.pragma library

// Depth-first objectName lookup, mirroring the Qt test utils' findChild.
// Returns null when no descendant carries `name`.
function findItem(root, name) {
    const found = findAllItems(root, name);
    return found.length > 0 ? found[0] : null;
}

// Every descendant of `root` whose objectName matches, in creation order
// (the same order the QML engine builds them in), which makes indexed
// access deterministic for repeated names like per-row rule fields.
function findAllItems(root, name) {
    if (root === null || root === undefined)
        return [];

    const matches = [];
    const stack = [root];
    while (stack.length > 0) {
        const item = stack.pop();
        // Visual children include everything Repeater instanced into the
        // list, so repeated rows are reachable by counting matches.
        const children = item.children || [];
        // Push in reverse so the stack pops them in their declared order:
        // a plain forward push would reverse a Repeater's delegates.
        for (let i = children.length - 1; i >= 0; i--)
            stack.push(children[i]);
        for (let i = 0; i < children.length; i++) {
            if (children[i].objectName === name)
                matches.push(children[i]);
        }
    }
    return matches;
}

// The replacement strings a getSuggestions() result offers, in order.
function suggestionValues(suggestions) {
    const values = [];
    for (let i = 0; i < suggestions.length; i++)
        values.push(suggestions[i].value);
    return values;
}

// Qt Quick Test's tryVerify takes no failure message in Qt 6.12; wrap the
// polling so a late condition still fails with the reason the caller wrote:
// pass the TestCase (`this` inside a test function) plus a predicate that
// returns the expected condition, and verify() the boolean it returns.
function hold(testCase, condition, timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        if (condition())
            return true;
        testCase.wait(25);
    }
    return condition();
}

// Record a step's outcome, then assert it.
//
// Two Qt 6.12 quirks force this order:
//   * `verify()` does not return its outcome (it is `undefined`), so
//     `var ok = verify(...)` reads as failure even for a passing step;
//   * a failing `verify()` aborts the test function, so a `capture()` placed
//     after it never runs — exactly when the image would be worth having.
// `captureWith`, when given, receives the boolean and saves the picture.
// Returns the condition so a later capture can reuse the same outcome.
// Use plain `verify()` for assertions that need no image.
function step(testCase, condition, message, captureWith) {
    if (captureWith)
        captureWith(condition);
    testCase.verify(condition, message);
    return condition;
}

// The QML test function currently executing. Quick Test keeps it on the
// internal result object, so every capture names the test that produced it
// instead of relying on a hand-maintained step number.
function currentTest(testCase) {
    const results = testCase.qtest_results;
    const name = results ? String(results.functionName) : "";
    return name ? name.replace(/[^A-Za-z0-9_]+/g, "_") : "unknown_test";
}

// Capture a screenshot of a QQuickItem (e.g. the checker text area) and
// save it to build/tests/ (sibling of the tests/ui/ directory that holds
// this file). The filename is `<suite>_<test>[_<label>]_<PASS|FAIL>.png`,
// with `<test>` taken from the QML function that called us. A window (e.g.
// ApplicationWindow) is grabbed through its content item; anything else is
// assumed to already be an item.
//
// Call once per test. A second shot from the same test needs a distinct
// `label` (e.g. "before"/"filled"), or the later file replaces the earlier.
function capture(testCase, passed, target, label) {
    var status = passed ? "PASS" : "FAIL";
    var filename = [testCase.name || "suite", currentTest(testCase)]
                       .concat(label ? [label] : [])
                       .concat([status]).join("_") + ".png";
    // Qt.resolvedUrl("./") in a .pragma library returns a Url whose toString()
    // is "file:///home/.../tests/ui/". Strip the protocol and go up two
    // levels (ui → tests → project root) to reach build/tests/.
    var urlStr = Qt.resolvedUrl("./").toString();
    // "file:///" → ""
    var base = urlStr.slice(7);
    // Remove trailing slash and the last two components (tests/ui)
    base = base.replace(/\/$/, "");
    var parts = base.split("/");
    parts.pop(); // remove "ui"
    parts.pop(); // remove "tests"
    var projectRoot = parts.join("/");
    var screenshotUrl = projectRoot + "/build/tests/" + filename;

    // grabImage takes a QQuickItem*. A window (ApplicationWindow, Window) is
    // not one: it owns a content item, and Qt ≥ 6.12 rejects the implicit
    // conversion outright ("Passing incompatible arguments to C++ functions").
    // Anything else is assumed to already be an item.
    if (!target) {
        // A missing target used to skip the save silently, which is how a
        // whole suite ended up with no images at all. Fail the step instead.
        testCase.fail("capture target for " + filename + " is null");
        return;
    }
    var grabTarget = target.contentItem ? target.contentItem : target;

    // grabImage() returns the last frame the scene graph produced, and a
    // frame is only produced when something grabs it. A single grab right
    // after a change can therefore still show the previous state. Grab once
    // to force a render, let the UI thread polish the change, then grab the
    // frame that documents the step. `waitForRendering()` never settles
    // here (the offscreen window is not exposed), so a bounded re-grab also
    // covers a page that was just pushed.
    testCase.grabImage(grabTarget);
    testCase.wait(100);
    var image = testCase.grabImage(grabTarget);
    for (var attempt = 0; attempt < 20 && image && looksUnrendered(image); attempt++) {
        testCase.wait(25);
        image = testCase.grabImage(grabTarget);
    }
    if (!image || !image.save) {
        testCase.fail("could not grab " + filename);
        return;
    }
    image.save(screenshotUrl);
    if (looksUnrendered(image)) {
        // A blank image named PASS hides exactly the state the step was
        // meant to document, so treat it as a harness failure.
        testCase.fail("screenshot " + filename + " is blank after 20 render attempts");
    }
}

// Whether a grab is still the bare background: one flat fill plus, at most,
// the toolbar separator. A rendered page carries text antialiasing and
// control chrome, so it exceeds this almost immediately. The 4px grid is
// dense enough to hit sparse text (the checker page is mostly empty), and
// the early exit keeps the common case cheap.
function looksUnrendered(image) {
    const seen = {};
    let distinct = 0;
    for (let y = 0; y < image.height; y += 4) {
        for (let x = 0; x < image.width; x += 4) {
            const key = String(image.pixel(x, y));
            if (seen[key] === undefined) {
                seen[key] = true;
                if (++distinct > 3)
                    return false;
            }
        }
    }
    return true;
}

// Parses "#rrggbb" / "#aarrggbb" (or any color object whose toString()
// renders "#rrggbb", e.g. QColor objects returned by pixel()) into [r, g, b].
function parseColor(color) {
    const digits = String(color).replace(/^#/, "");
    const start = digits.length === 8 ? 2 : 0; // skip the alpha part
    return [
        parseInt(digits.slice(start, start + 2), 16),
        parseInt(digits.slice(start + 2, start + 4), 16),
        parseInt(digits.slice(start + 4, start + 6), 16)
    ];
}

// Whether the pixel at (x, y) sits within `tolerance` per channel of
// `target` — tight enough to not mistake every red-ish background for an
// underline, loose enough to survive the antialiasing of a thin line.
function colorNear(image, x, y, target, tolerance) {
    const channels = parseColor(image.pixel(x, y));
    for (let i = 0; i < 3; i++) {
        if (Math.abs(channels[i] - target[i]) > tolerance)
            return false;
    }
    return true;
}

// Scans the first `maxRows` rows of a grabImage() result (full width) for
// any pixel near `target`, with early exit: the suites only need presence,
// and the painted elements they look for live near the top of the page.
function imageHasColorNear(image, target, maxRows, tolerance) {
    const channels = parseColor(target);
    const height = Math.min(maxRows, image.height);
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < image.width; x++) {
            if (colorNear(image, x, y, channels, tolerance))
                return true;
        }
    }
    return false;
}
