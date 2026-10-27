# Test Policy

## New functionality

Each change that adds major new functionality must add tests for that
functionality to the automated test suite. Include the tests in the same change
proposal as the implementation.

Major new functionality includes new commands, language support, metrics,
reference resolution, and report formats.

Follow [AGENTS.md](AGENTS.md):

- Write the required test before you implement the behavior.
- Put tests in `tests/`. Do not put tests in application source files.
- Keep tests short, simple, and reusable.

Test the expected behavior, relevant error conditions, and boundary conditions.
Use small inputs and check results that a user can observe. Reuse test support
code where practical. Do not add tests that repeat existing checks without a
clear purpose.

Changes to documentation alone do not require new tests.

## Instructions for change proposals

Before you request review of major new functionality:

1. Describe the new behavior.
2. Identify the added tests and the behavior that each test checks.
3. Run the automated Rust test suite from the repository root:

   ```sh
   cargo test --locked
   ```

4. Record the command and its result in the change proposal. State any tests
   that you did not run and give the reason.

Maintainers must check that the proposal includes tests for the new
functionality before they merge it. Resolve missing tests and test failures
before merge. Keep the test paths and results in the change proposal so that
later reviews can check whether this policy was followed.
