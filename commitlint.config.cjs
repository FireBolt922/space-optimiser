// Conventional Commits (https://www.conventionalcommits.org/en/v1.0.0/).
// The PR title is checked with the same rules in CI, because PRs are
// squash-merged and the title becomes the commit on main.
module.exports = {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "type-enum": [
      2,
      "always",
      ["feat", "fix", "docs", "test", "refactor", "perf", "build", "ci", "chore", "revert"],
    ],
    "scope-enum": [2, "always", ["engine", "desktop", "ci", "docs", "release", "deps", "hooks"]],
    "scope-empty": [0],
    "header-max-length": [2, "always", 72],
    "body-max-line-length": [2, "always", 100],
    "subject-case": [2, "never", ["sentence-case", "start-case", "pascal-case", "upper-case"]],
    "subject-full-stop": [2, "never", "."],
  },
};
