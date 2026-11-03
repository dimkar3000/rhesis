// Page-level suite for the checker flow: service status, recorded answers
// becoming painted recommendations, and the clean-text path.
//
// The shell inside mirrors Root.qml's object graph (same hook order —
// highlighter first, helper second) and connects to the runner's fake
// daemon (spooled by ui_tests.rs :: run_suites through RHESIS_SERVICE_SOCKET),
// so no LanguageTool is ever spawned: requests answer from the fixture
// table in src/fake_daemon.rs.
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

    // Worker status codes (WorkerStatus::code, rhesis-protocol):
    readonly property int statusStarted: 2

    // Fixture constants — the recorded field text and the ranges it reports
    // for the two typo groups ("like" [4,8), "quicly" [38,44)).
    readonly property string fixtureText: "She like football and play the guitar quicly."
    readonly property string cleanText: "This sentence has no problems."

    // Color the fixture stamps on its recommendations ("#FF0000" on the
    // wire; QML compares lowercase hex to the image pixels).
    readonly property string pipelineDefaultColor: "#ff0000"

    Kirigami.ApplicationWindow {
        id: appShell

        visible: false
        width: 1280
        height: 720

        // Declaration order matters: the highlighter wires the connection to
        // the runner's fake daemon before the helper completes — as in
        // Root.qml. The daemon drives the mirror from here: its handshake
        // replay (snapshot + status) fills the helper properties, and every
        // check answer flows back through the fan-out.
        CustomHighlighter {
            id: highlighter
            Component.onCompleted: highlighter.startMessageThread(messagingHelper)
        }

        AsyncMessagingHelper {
            id: messagingHelper
        }

        pageStack.initialPage: MainPage {
            id: mainPage
            highlighter: highlighter
            helper: messagingHelper
        }
    }

    function textArea() {
        return Harness.findItem(appShell.pageStack, "checkerTextArea");
    }

    function test_fake_service_replays_started() {
        Harness.step(this, Harness.hold(this,
                            () => messagingHelper.server_status === statusStarted, 5000),
               "the fake daemon's handshake replay reads Started without any server",
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
