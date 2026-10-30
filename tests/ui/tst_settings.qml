// Page-level suite for the settings flow: the draft contract (edits wait
// for Apply), the autostart⇄background pairing lock, and the color-rule
// rows feeding the committed colorSettings.
//
// The helper is intentionally left unstarted (no restart → no worker churn):
// only the autostart and background helpers are exercised here.
//
// The runner sandboxed every XDG home, so the store and the autostart
// desktop entry that Apply commits live in the harness's own config root.
// Interactions go through the user path — click() for switches and buttons —
// because direct property writes do not run the page's toggled/clicked
// handlers, and toggle() does not emit toggled for bound switches either.
// Quick Test loads this file into a
// QQuickView, so the root stays a plain item (TestCase) and the app shell
// lives below it as a nested window.
import QtCore
import QtQuick
import QtTest
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis
import "harness.js" as Harness

TestCase {
    name: "settings"

    // Production pages resolve unqualified `applicationWindow()` through
    // their file root (Root.qml). Pages created from this file instead fall
    // back to *this* root object, so expose the shell the same way — the
    // Apply handler uses it for its tray-aware visibility fix-up and for
    // the trailing layer pop.
    function applicationWindow() {
        return appShell;
    }

    // QtCore Settings wants an application identity the harness process does
    // not have — pointing the store at an explicit file via the documented
    // `location` property makes that lookup unnecessary.
    readonly property string settingsFile:
        StandardPaths.writableLocation(StandardPaths.GenericConfigLocation)
            + "/rhesis-ui-tests-settings.conf"

    Kirigami.ApplicationWindow {
        id: appShell

        visible: false
        width: 1280
        height: 720

        // Root.qml derives this from the process argv; harness runs never
        // pass --hidden, so the Apply handler's visibility fix-up stays a
        // no-op.
        readonly property bool hiddenLaunch: false

        // The same store shape as Root.qml. The file is truncated with the
        // sandbox each run, so the first read returns these defaults.
        Settings {
            id: appSettings
            location: settingsFile
            property bool embedded: true
            property string port: "2689"
            property string defaultPort: "2689"
            property string externalAddress: ""
            property bool background: false
            property bool showTooltips: true
            property bool showDebugTooltips: true
            property string language: ""
            property var colorSettings: ({
                    "CATEGORY:GRAMMAR": "#ecc224",
                    "CATEGORY:TYPOGRAPHY": "#f63ef9",
                    "CATEGORY:TYPOS": "#1ae918"
                })
        }

        LanguageManager {
            id: languageManager
        }

        // No Component.onCompleted: the suite never (re)starts the fake
        // worker; only the autostart and background helpers get exercised.
        AsyncMessagingHelper {
            id: messagingHelper
        }

        pageStack.initialPage: Kirigami.Page {
        }

        // The layer push/pop transitions animate `y` with a YAnimator, i.e.
        // on the scene graph's render thread. Under the offscreen platform
        // frames only exist when something grabs them, so the animation
        // stalls and a screenshot catches the page half-slid into place.
        // These suites assert layout and text, never motion, so make the
        // transitions instant.
        pageStack.layers.pushEnter: Transition {}
        pageStack.layers.pushExit: Transition {}
        pageStack.layers.popEnter: Transition {}
        pageStack.layers.popExit: Transition {}

        // The production layer push: same call shape as Root's settings
        // alias, so Apply's trailing pageStack.layers.pop() behaves exactly
        // as in the app (the popped layer instance is destroyed).
        Component {
            id: settingsPageComponent
            SettingsPage {
                settings: appSettings
                helper: messagingHelper
                langManager: languageManager
            }
        }
    }

    function initTestCase() {
        verify(!messagingHelper.is_autostart(),
               "the sandboxed config root ships without an autostart entry");
    }

    // QuickTest runs the functions alphabetically, not in file order, so
    // every entry point resets the state an earlier function could have
    // persisted: remove the autostart entry and write the store's defaults
    // back. Factories for page state still seed what they need explicitly
    // (see the removal test).
    function init() {
        if (messagingHelper.is_autostart())
            messagingHelper.set_autostart(false);
        appSettings.background = false;
        appSettings.showTooltips = true;
        appSettings.showDebugTooltips = true;
        appSettings.colorSettings = {
            "CATEGORY:GRAMMAR": "#ecc224",
            "CATEGORY:TYPOGRAPHY": "#f63ef9",
            "CATEGORY:TYPOS": "#1ae918"
        };
    }

    // Pages are pushed as layers so they resolve `applicationWindow()` and
    // pop exactly like production. Look-ups later stay scoped to this page
    // instance: old layers can remain stacked invisibly, and objectNames
    // repeat across them.
    function pushPage() {
        return appShell.pageStack.layers.push(settingsPageComponent);
    }

    function popPage() {
        appShell.pageStack.layers.pop();
    }

    // The rule rows live below the fold (the page is taller than the 720px
    // window cap). Scroll the page down so a capture actually shows them.
    function showRules(page) {
        var flickable = Harness.findItem(page, "settingsFlickable");
        verify(flickable !== null, "the settings page is scrollable");
        flickable.contentY = Math.max(0, flickable.contentHeight - flickable.height);
    }

    // The page has exactly one action: Apply.
    function applyAction(page) {
        return page.actions[0];
    }

    // Emitting `triggered` is the exact QML signal the toolbar button fires;
    // every commit (settings writes, autostart desktop files, background
    // host state) happens synchronously inside the handler, and the page
    // pops at its end.
    function apply(page) {
        page.actions[0].triggered();
    }

    function test_fresh_page_keeps_apply_disabled() {
        var page = pushPage();
        Harness.step(this, page !== null, "a new settings layer opens",
                (ok) => Harness.capture(this, ok, appShell));
        compare(page.actions.length, 1, "one action lives on the page");
        verify(!applyAction(page).enabled, "nothing edited yet: no draft is pending");
        compare(page.localTooltipSwitch, appSettings.showTooltips,
                "the switch drafts mirror the persisted settings on open");
        popPage();
    }

    function test_tooltip_switch_edits_a_draft_until_apply() {
        var page = pushPage();
        var tooltip = Harness.findItem(page, "tooltipSwitch");
        verify(tooltip !== null, "the tooltip switch is reachable");
        compare(appSettings.showTooltips, true, "the store default");

        compare(tooltip.checked, true, "the switch mirrors the draft on open");
        tooltip.click(); // the user path (press+release); page.onToggled joins the draft
        compare(page.localTooltipSwitch, false, "the click joins the draft");
        compare(appSettings.showTooltips, true, "the persisted state waits for Apply");
        Harness.step(this, Harness.hold(this, () => applyAction(page).enabled, 5000),
               "a pending draft enables Apply",
               (ok) => Harness.capture(this, ok, appShell));

        apply(page); // synchronous commits, then the layer pops
        compare(appSettings.showTooltips, false, "the draft is committed");
        compare(appSettings.showDebugTooltips, false,
                "debug tooltips pair with the master tooltip switch");
    }

    function test_autostart_draft_pairs_and_locks_background_switch() {
        var page = pushPage();
        var autostart = Harness.findItem(page, "autostartSwitch");
        var background = Harness.findItem(page, "backgroundSwitch");
        verify(autostart !== null && background !== null, "both switches reachable");
        verify(background.enabled, "embedded default: the background switch is unlocked");
        compare(background.checked, false, "nothing forces hosting yet");

        autostart.click();
        compare(page.localAutostartSwitch, true, "the draft knows the request");
        compare(background.checked, true, "autostart forces background hosting on");
        Harness.step(this, !background.enabled, "the forced pairing locks the switch",
                (ok) => Harness.capture(this, ok, appShell));

        autostart.click();
        compare(page.localAutostartSwitch, false, "the draft releases the request");
        verify(background.enabled, "unpairing releases the lock");
        compare(background.checked, true,
                "the forced value stays for the user to adjust");
        popPage();
    }

    function test_apply_commits_the_autostart_background_pairing() {
        var page = pushPage();
        var autostart = Harness.findItem(page, "autostartSwitch");
        autostart.click();
        Harness.step(this, Harness.hold(this, () => applyAction(page).enabled, 5000),
               "a pairing draft is pending",
               (ok) => Harness.capture(this, ok, appShell));

        apply(page);
        // Everything below lives outside the (popped) layer; the commits
        // finished synchronously before the page popped.
        verify(messagingHelper.is_autostart(),
               "Apply wrote the desktop entry: the file-based check reads it back");
        compare(appSettings.background, true,
                "the paired background draft was committed with it");
    }

    function test_later_apply_removes_the_autostart_entry() {
        // Self-contained precondition: the entry could come from anywhere
        // (a previous session, a previous test), so seed it ourselves.
        verify(messagingHelper.set_autostart(true), "the sandbox accepts an entry");

        var page = pushPage();
        var autostart = Harness.findItem(page, "autostartSwitch");
        // Seeded from the on-disk state, not from the persisted settings.
        compare(autostart.checked, true, "the switch mirrors the on-disk entry");

        autostart.click();
        Harness.step(this, Harness.hold(this, () => applyAction(page).enabled, 5000),
               "back-office state differs from the draft",
               (ok) => Harness.capture(this, ok, appShell));

        apply(page);
        verify(!messagingHelper.is_autostart(), "Apply removed the desktop entry");
        compare(appSettings.background, true,
                "background keeps the forced-on draft, untouched by the removal");
    }

    function test_rule_rows_join_the_color_rules_draft() {
        var page = pushPage();
        var add = Harness.findItem(page, "addRuleButton");
        verify(add !== null, "the add-rule button is reachable");
        compare(Harness.findAllItems(page, "ruleTextField").length, 3,
                "the Root defaults list three color rules");
        showRules(page);
        Harness.step(this, !applyAction(page).enabled,
                "an unedited page cannot be applied",
                (ok) => Harness.capture(this, ok, appShell, "before"));

        add.click(); // the user path; onClicked appends a row
        var rows = Harness.findAllItems(page, "ruleTextField");
        compare(rows.length, 4, "adding appends an empty row");
        var empty = rows.filter(row => row.text === "");
        compare(empty.length, 1, "exactly the fresh row is empty");
        verify(!applyAction(page).enabled,
               "an empty rule text keeps the form unsubmitable");

        empty[0].text = "RULETEST"; // editing the row updates the draft model
        verify(Harness.hold(this, () => applyAction(page).enabled, 5000),
               "a complete draft enables Apply");

        Harness.capture(this, applyAction(page).enabled, appShell, "filled");
        apply(page);
        var committed = appSettings.colorSettings;
        compare(committed["CATEGORY:RULETEST"], "#000000",
                "Apply persisted the appended rule with its draft color");
    }

    function test_removing_a_rule_row_cancels_the_addition() {
        var page = pushPage();
        showRules(page);
        Harness.findItem(page, "addRuleButton").click();
        var rows = Harness.findAllItems(page, "ruleTextField");
        compare(rows.length, 4, "one row is added for the removal scenario");

        // Creation order equals model order, so the appended row and its
        // remove button are both the last of their kind.
        rows[3].text = "RULETEST";
        var remove = Harness.findAllItems(page, "removeRuleButton")[3];
        remove.click();
        showRules(page);
        // `compare()` does not return its outcome either, so evaluate the
        // condition once and feed both the screenshot and the assertion.
        var remaining = Harness.findAllItems(page, "ruleTextField").length;
        Harness.step(this, remaining === 3, "the row leaves the draft on request",
                (ok) => Harness.capture(this, ok, appShell));
        verify(!applyAction(page).enabled,
               "equal draft and saved shapes: nothing left to commit");
        popPage();
    }

    function cleanupTestCase() {
        // Leave the sandbox as we found it, whatever a failing intermediate
        // test left behind.
        if (messagingHelper.is_autostart())
            verify(messagingHelper.set_autostart(false), "sandbox autostart removed");
        verify(!messagingHelper.is_autostart(), "the sandbox autostart is clean");
    }
}
