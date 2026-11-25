#!/usr/bin/env python3
"""Regression tests for the required CLA check."""
import html
import runpy
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
checker = runpy.run_path(str(SCRIPTS / "check-contribution-license.py"))
TEMPLATE = (SCRIPTS.parent / ".github/pull_request_template.md").read_text()
OWNER = "metaneutrons"


def user(login):
    return {"login": login, "type": "Bot" if login.endswith("[bot]") else "User"}


def commit(login, trailers=""):
    return {
        "sha": "0123456789",
        "author": user(login) if login else None,
        "commit": {"author": {"email": f"{login}@example.org"}, "message": "fix: example\n\n" + trailers},
    }


def task(text, checked=True):
    flag = ' checked=""' if checked else ""
    return f'<ul><li><input type="checkbox"{flag}> {text}</li></ul>'


def problems(opener, commits, body="", count=None, fork=False):
    pr = {
        "user": user(opener), "commits": len(commits) if count is None else count, "body_html": body,
        "head": {"repo": {"id": 2 if fork else 1}}, "base": {"repo": {"id": 1}},
    }
    return checker["check"](pr, commits, {OWNER})


class ContributionLicenceTests(unittest.TestCase):
    def test_template_requires_an_affirmative_versioned_declaration(self):
        commits = [commit("alice")]
        declaration = checker["DECLARATION"]
        self.assertIn("- [ ] " + declaration, TEMPLATE.splitlines())
        self.assertFalse(problems("alice", commits, task(declaration), fork=True))
        for rejected in (
            task(declaration, checked=False),
            task(declaration.replace("v1", "v10")),
            task("I decline CONTRIBUTION-LICENSE v1"),
            f"<blockquote>{task(declaration)}</blockquote>",
            f"<!-- {task(declaration)} -->",
            f"<pre><code>{html.escape('- [x] ' + declaration)}</code></pre>",
        ):
            with self.subTest(body=rejected):
                self.assertTrue(problems("alice", commits, rejected))

    def test_previous_v1_declarations(self):
        for key in ("OLD_TEMPLATE", "OLD_DECLARATION"):
            self.assertFalse(problems("alice", [commit("alice")], task(checker[key])))

    def test_maintainer_and_automation_only_exempt_their_own_work(self):
        for opener in (OWNER, "dependabot[bot]", "metaneutrons-release-please[bot]"):
            with self.subTest(opener=opener):
                self.assertFalse(problems(opener, [commit(opener)]))
                self.assertTrue(problems(opener, [commit(opener), commit("alice")]))
                signed = commit("alice", "Signed-off-by: Alice <alice@example.org>")
                self.assertFalse(problems(opener, [commit(opener), signed]))
        self.assertTrue(problems("other[bot]", [commit("other[bot]")], task(checker["DECLARATION"])))
        self.assertTrue(problems("github-actions[bot]", [commit("github-actions[bot]")], fork=True))

    def test_each_author_and_coauthor_accepts_on_a_shared_commit(self):
        body = task(checker["DECLARATION"])
        coauthor = "Co-authored-by: Bob <bob@example.org>"
        alice = "Signed-off-by: Alice <alice@example.org>"
        bob = "Signed-off-by: Bob <bob@example.org>"
        for signoff in [alice, bob]:
            self.assertTrue(problems("alice", [commit("alice", coauthor + "\n" + signoff)], body))
        self.assertFalse(problems("alice", [commit("alice", "\n".join((coauthor, alice, bob)))], body))
        self.assertTrue(problems(OWNER, [commit(OWNER, coauthor)]))
        self.assertTrue(problems(OWNER, [commit(OWNER, "Co-authored-by: Helper [bot] <bob@example.org>")]))

    def test_unlinked_author_and_incomplete_api_results(self):
        self.assertTrue(problems(OWNER, [commit(None)]))
        self.assertTrue(problems(OWNER, [commit(OWNER)], count=2))


if __name__ == "__main__":
    unittest.main()
