// Page-level suite for the checker flow: worker status, recorded answers
// becoming painted recommendations, and the clean-text path.
//
// The shell inside mirrors Root.qml's object graph (same hook order —
// highlighter first, helper second) and runs the hermetic fake provider
// seeded by the runner (`RHESIS_FAKE_LT=checker`), so no LanguageTool is
// ever spawned: requests answer from tests/fixtures/lt/checker.json.
//
// Qt Quick Test loads this file into a QQuickView, so the root must remain
// a plain item (TestCase) — the app shell lives below it as a nested,
// deliberately unshown window.
import QtQuick
import QtTest
import org.kde.kirigami as Kirigami
import io.github.dimkar3000.rhesis
import "harness.js" as Harness

TestCase {
    name: "checker"

    // Worker status codes (WorkerStatus::code, languagetool/service.rs):
    readonly property int statusStarted: 2

    // Fixture constants — the recorded field text and the ranges it reports
    // for the two typo groups ("like" [4,8), "quicly" [38,44)).
    readonly property string fixtureText: "She like football and play the guitar quicly."
    readonly property string cleanText: "This sentence has no problems."

    // Color the pipeline stamps on recommendations when no user rules are
    // known — the fake builds its answers with an empty rule table, so this
    // is the color every fixture rec paints (client.rs select_color default).
    readonly property string pipelineDefaultColor: "#ff0000"

    Kirigami.ApplicationWindow {
        id: appShell

        visible: false
        width: 1280
        height: 720

        // Settings store: only what MainPage consumes; colorSettings carries
        // the Root.qml defaults so update_colors runs like production does.
        QtObject {
            id: appSettings
            property bool showTooltips: true
            property bool showDebugTooltips: true
            property var colorSettings: ({
                    "CATEGORY:GRAMMAR": "#ecc224",
                    "CATEGORY:TYPOGRAPHY": "#f63ef9",
                    "CATEGORY:TYPOS": "#1ae918"
                })
        }

        // Declaration order matters: the highlighter wires the message
        // fan-out (which also starts the fake worker) before the helper
        // completes and requests the local server — as in Root.qml.
        CustomHighlighter {
            id: highlighter
            Component.onCompleted: highlighter.startMessageThread(messagingHelper)
        }

        AsyncMessagingHelper {
            id: messagingHelper
            Component.onCompleted: {
                messagingHelper.restart(true, "2689");
                messagingHelper.update_colors(appSettings.colorSettings);
            }
        }

        pageStack.initialPage: MainPage {
            id: mainPage
            highlighter: highlighter
            helper: messagingHelper
            settings: appSettings
        }
    }

    function textArea() {
        return Harness.findItem(appShell.pageStack, "checkerTextArea");
    }

    function test_fake_worker_reaches_started() {
        Harness.step(this, Harness.hold(this,
                            () => messagingHelper.server_status === statusStarted, 5000),
               "the fake provider boots the worker into Started without any server",
               (ok) => Harness.capture(this, ok, textArea()));
    }

    function test_recommendations_follow_the_fixture() {
        textArea().text = fixtureText;

        // Inside "like" (index 6): covers [4,8) like the recorded match.
        Harness.step(this, Harness.hold(this,
                            () => highlighter.findRecommendation(6).length === 2, 5000),
               "typing the recorded text produces a recommendation at [4,8]",
               (ok) => Harness.capture(this, ok, textArea()));
        var like = highlighter.findRecommendation(6);
        compare(like[0], 4, "grammar group starts at 'like'");
        compare(like[1], 8, "end is exclusive: start + length");

        // The context-menu path: MainPage calls getSuggestions(start, end)
        // — both fixture groups pass the length-based filter (6 <= 8), so
        // the recorded replacements of BOTH groups come back, in fixture
        // order.
        var inside = Harness.suggestionValues(
            highlighter.getSuggestions(like[0], like[1]));
        compare(inside, ["is like", "likes", "quickly"],
                "the menu lists every recorded replacement admitted by the filter");

        // Between words (exclusive end): nothing covers index 8.
        compare(highlighter.findRecommendation(8).length, 0, "gaps have no group");

        // Inside "quicly" (index 38): the first group starts before the
        // requested start, so only the spelling group is offered.
        var quicly = highlighter.findRecommendation(38);
        compare(quicly[0], 38, "typo group starts at 'quicly'");
        compare(quicly[1], 44, "typo group ends at the period");
        compare(highlighter.getSuggestions(38, 44).map(r => r.value), ["quickly"],
                "clicking a later group offers only its own replacements");
    }

    function test_highlighter_paints_the_pipeline_color() {
        textArea().text = fixtureText;
        verify(Harness.hold(this,
                            () => highlighter.findRecommendation(6).length === 2, 5000),
               "suggestions landed before rendering is asserted");

        const image = grabImage(textArea());
        verify(image !== null, "the text area rendered under offscreen");
        Harness.step(this, Harness.imageHasColorNear(image, pipelineDefaultColor, 200, 40),
                "the underlined words carry the pipeline's default color",
                (ok) => Harness.capture(this, ok, textArea()));
    }

    function test_clean_text_clears_recommendations() {
        textArea().text = cleanText;

        // Empty answers replace the previous recommendation set; waiting on
        // the full-span scan is the one-true signal that they replaced it.
        var spansClean = () => {
            for (let pos = 0; pos < cleanText.length; pos++) {
                if (highlighter.findRecommendation(pos).length !== 0)
                    return false;
            }
            return highlighter.getSuggestions(0, 999).length === 0;
        };
        Harness.step(this, Harness.hold(this, spansClean, 5000),
               "a recorded clean answer removes all matches",
               (ok) => Harness.capture(this, ok, textArea()));
    }
}
