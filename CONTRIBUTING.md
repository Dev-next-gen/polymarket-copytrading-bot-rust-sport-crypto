# Contributing to Polymarket Copy Trading Bot

Thank you for your interest in contributing to this project! This document provides guidelines for contributing to the Polymarket Copy Trading Bot.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally
3. **Set up the development environment**:
   ```bash
   # Install Rust
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Install required tools
   cargo install trunk
   rustup target add wasm32-unknown-unknown
   ```

## Development Guidelines

### Code Style

- Follow Rust's official style guidelines (use `cargo fmt`)
- Use `cargo clippy` to catch common issues
- Write comprehensive documentation for public APIs
- Include examples in documentation when helpful

### Testing

- Write unit tests for all new utility functions
- Test edge cases and error conditions
- Run the full test suite before submitting: `cargo test`
- Ensure all tests pass in both debug and release modes

### Documentation

- Use rustdoc comments for all public functions and modules
- Include usage examples in documentation
- Update the changelog for all notable changes
- Keep README.md up to date with new features

### Commit Messages

Follow the conventional commit format:

```
type(scope): description

[optional body]

[optional footer]
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

Examples:
```
feat(utils): add Ethereum address validation utilities
fix(trading): handle edge case in error retry logic  
docs(readme): update installation instructions
```

## Code Quality Standards

### Error Handling
- Use `anyhow::Result` for functions that can fail
- Provide meaningful error messages with context
- Document error conditions in function docs
- Handle errors gracefully without panicking

### Performance
- Avoid unnecessary allocations in hot paths
- Use appropriate data structures for the use case
- Profile performance-critical code changes
- Document performance characteristics

### Security
- Validate all external inputs
- Use secure random number generation where needed
- Avoid logging sensitive information
- Follow Rust security best practices

## Pull Request Process

1. **Create a feature branch**: `git checkout -b feature/your-feature-name`
2. **Make your changes** following the guidelines above
3. **Add tests** for new functionality
4. **Update documentation** as needed
5. **Run the test suite**: `cargo test`
6. **Run formatting and linting**:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```
7. **Update CHANGELOG.md** with your changes
8. **Submit a pull request** with:
   - Clear title and description
   - Reference to any related issues
   - Summary of changes made
   - Testing instructions

### PR Review Process

All PRs will be reviewed for:
- Code quality and style adherence
- Test coverage and quality
- Documentation completeness
- Performance implications
- Security considerations
- Backward compatibility

## Reporting Issues

When reporting bugs or requesting features:

1. **Check existing issues** to avoid duplicates
2. **Use appropriate labels** (bug, enhancement, documentation, etc.)
3. **Provide clear reproduction steps** for bugs
4. **Include relevant system information** (OS, Rust version, etc.)
5. **Attach logs or error messages** when relevant

### Bug Report Template

```markdown
**Describe the bug**
A clear description of what the bug is.

**To Reproduce**
Steps to reproduce the behavior:
1. Go to '...'
2. Click on '....'
3. See error

**Expected behavior**
What you expected to happen.

**Environment**
- OS: [e.g. Ubuntu 20.04]
- Rust version: [e.g. 1.70.0]
- Bot version: [e.g. 0.1.0]

**Additional context**
Any other context about the problem.
```

## Development Setup

### Local Testing

1. **Create test configuration files**:
   ```bash
   cp config.example.json config.json
   cp trade.example.toml trade.toml
   # Edit with your test values
   ```

2. **Run in simulation mode**:
   ```bash
   cargo run --release -- --simulation
   ```

3. **Build frontend for testing**:
   ```bash
   cd frontend && trunk build --release && cd ..
   ```

### Useful Commands

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Generate documentation
cargo doc --open

# Check for issues
cargo clippy

# Format code
cargo fmt

# Build release
cargo build --release

# Run with logs
RUST_LOG=debug cargo run --release
```

## Community

- Be respectful and inclusive
- Help others learn and grow
- Share knowledge and best practices
- Follow the project's code of conduct

## Questions?

If you have questions about contributing, feel free to:
- Open an issue for discussion
- Contact the maintainers
- Check the documentation

Thank you for contributing to make this project better!