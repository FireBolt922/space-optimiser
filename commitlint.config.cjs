// Conventional Commits (https://www.conventionalcommits.org/en/v1.0.0/).
// Only the pull request title is checked (CI), because PRs are
// squash-merged and the title becomes the commit on main. Commits on a
// branch can say anything. The type is what the release workflow reads to
// pick the next version, so the type list stays; everything else is loose.
module.exports = {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "type-enum": [
      2,
      "always",
      ["feat", "fix", "docs", "test", "refactor", "perf", "build", "ci", "chore", "revert"],
    ],
    // Any scope, or none.
    "scope-enum": [0],
    "scope-case": [0],
    "scope-empty": [0],
    // "Add x" and "add x" are both fine.
    "subject-case": [0],
    "subject-full-stop": [0],
    // 100, not 72: Dependabot's group titles run to about 85 characters.
    "header-max-length": [2, "always", 100],
    // Bodies hold links and logs; they are not linted.
    "body-max-line-length": [0],
    "footer-max-line-length": [0],
  },
};
