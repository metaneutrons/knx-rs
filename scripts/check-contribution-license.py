#!/usr/bin/env python3
"""Check that a pull request carries the contribution licence declaration.

Usage: check-contribution-license.py PR.json COMMITS.json

PR.json      the pull request object of the GitHub API
COMMITS.json the list of commits of the pull request (GitHub API, all pages merged)

Environment: MAINTAINERS, comma separated GitHub logins that own the project and need no declaration.

Standard library only. Exit status 1 on a missing declaration.
"""
import json
import os
import re
import sys
from html.parser import HTMLParser

DECLARATION = (
    "I accept CONTRIBUTION-LICENSE v1 for my contribution to this pull request, "
    "including its use in releases without copyleft."
)
# Accept the two declarations previously supplied by the v1 template and checker.
OLD_TEMPLATE = (
    "I have read the Contribution Licence (Deutsch) (CONTRIBUTION-LICENSE v1) "
    "and license this contribution under its terms."
)
OLD_DECLARATION = (
    "I have read CONTRIBUTION-LICENSE.md (CONTRIBUTION-LICENSE v1) "
    "and license this contribution under its terms."
)
SIGNED = re.compile(r"^Signed-off-by:[ \t]*[^<>\n]*<([^<>\s]+)>[ \t]*$", re.M | re.I)
COAUTHOR = re.compile(r"^Co-authored-by:[ \t]*([^<>\n]*?)\s*<([^<>\s]+)>[ \t]*$", re.M | re.I)
PROJECT_BOTS = {"dependabot[bot]", "github-actions[bot]", "metaneutrons-release-please[bot]"}


class CheckedItems(HTMLParser):
    """Read checked task items from GitHub's rendered PR body."""

    def __init__(self):
        super().__init__()
        self.items = []
        self.text = None
        self.checked = False
        self.quote_depth = 0

    def handle_starttag(self, tag, attrs):
        if tag == "blockquote":
            self.quote_depth += 1
        elif tag == "li":
            self.text = []
            self.checked = False
        elif tag == "input" and self.text is not None:
            attrs = dict(attrs)
            self.checked = attrs.get("type") == "checkbox" and "checked" in attrs

    def handle_data(self, data):
        if self.text is not None:
            self.text.append(data)

    def handle_endtag(self, tag):
        if tag == "blockquote":
            self.quote_depth -= 1
        elif tag == "li" and self.text is not None:
            if self.checked and not self.quote_depth:
                self.items.append(" ".join("".join(self.text).split()))
            self.text = None


def check(pr, commits, maintainers):
    opener = pr["user"]
    same_repo = (pr["head"].get("repo") or {}).get("id") == pr["base"]["repo"]["id"]

    def exempt(user):
        login = user.get("login")
        return login in maintainers or (same_repo and user.get("type") == "Bot" and login in PROJECT_BOTS)

    problems = []
    if pr["commits"] != len(commits):
        problems.append("The commit list is incomplete. Every commit must be available for the CLA check.")
    if not exempt(opener):
        if opener.get("type") == "Bot":
            problems.append("A human contributor must submit this PR and accept the CLA for the material.")
        rendered = CheckedItems()
        rendered.feed(pr["body_html"] or "")
        rendered.close()
        if not any(item in (DECLARATION, OLD_TEMPLATE, OLD_DECLARATION) for item in rendered.items):
            problems.append("Read the CLA and tick its declaration in the PR description:\n- [x] " + DECLARATION)

    multiple_authors = any(
        (c.get("author") or {}).get("login") != opener["login"] or COAUTHOR.search(c["commit"]["message"])
        for c in commits
    )
    for c in commits:
        user = c.get("author") or {}
        msg = c["commit"]["message"]
        email = c["commit"]["author"]["email"]
        signed = {mail.casefold() for mail in SIGNED.findall(msg)}
        short = c["sha"][:7]
        if multiple_authors and not exempt(user) and email.casefold() not in signed:
            problems.append(f"Commit {short} needs the author's Signed-off-by line for {email}.")
        for name, mail in COAUTHOR.findall(msg):
            if mail.casefold() not in signed:
                problems.append(f"Commit {short} needs the co-author's Signed-off-by line for {name} <{mail}>.")
    return problems


def main(argv):
    if len(argv) != 3:
        sys.exit(__doc__)
    with open(argv[1], encoding="utf-8") as f:
        pr = json.load(f)
    with open(argv[2], encoding="utf-8") as f:
        commits = json.load(f)
    maintainers = {m.strip() for m in os.environ.get("MAINTAINERS", "").split(",") if m.strip()}
    problems = check(pr, commits, maintainers)
    for p in problems:
        print("::error::" + p.replace("\n", "%0A"))
    if problems:
        print("See CONTRIBUTION-LICENSE.md.")
        return 1
    print("Contribution licence declaration present.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
