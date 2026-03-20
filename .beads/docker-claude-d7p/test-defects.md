# Test Defects: fastest-claude-dock

## 1. Dave Farley ATDD - Leaky Assertions
- **Defect**: Unit tests (lines 4-11) assert on exact string matching of `docker run` arguments (e.g., `Contains -v ...`).
- **Remedy**: Refactor these to be behavior-focused. Use a DSL to assert that "the project directory is correctly mapped" or "the container runs as the host user," separating the *intent* from the *implementation* of the command string.

## 2. Incomplete Requirement Coverage (Mounts)
- **Defect**: The contract (line 35) requires `.gitconfig` and `.jj` to be available in the container. The test plan (lines 7, 75) only verifies the `.claude` mount.
- **Remedy**: Add specific test cases to verify that all required configuration files (`.gitconfig`, `.jj`) are correctly mounted into the container's home directory.

## 3. Missing Build Failure Handling
- **Defect**: `Error::ImageBuildFailed` is defined in the contract's error taxonomy (line 50) but is not covered by any test case in the plan.
- **Remedy**: Add an error path test `test_returns_error_when_image_build_fails` to verify graceful handling of build errors.

## 4. Startup Speed Verification Proxy
- **Defect**: The test plan uses `docker run ... --version` (line 68) as a proxy for the contract's requirement of "reaching the Claude prompt" (line 36).
- **Remedy**: Implement a test that measures the time to reach an interactive state or a specific prompt string to ensure the < 100ms requirement is met for the actual use case, not just a minimal execution.

## 5. Under-utilized Property-Based Testing
- **Defect**: Property-based testing is limited to volume path mapping (line 72).
- **Remedy**: Expand PBT to cover the construction of the entire command vector, including handling of arbitrary UID/GID values and varied extra arguments, ensuring the output is always a valid and safe Docker command.
