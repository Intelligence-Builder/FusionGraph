//! Circuit breaker pattern for external dependency protection.
//!
//! Implements the circuit breaker pattern to prevent cascading failures
//! when external dependencies (Iceberg, Snowflake, etc.) become unavailable.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::Duration;

use crate::error::{GraphError, Result};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally.
    Closed = 0,
    /// Circuit is open, requests fail immediately.
    Open = 1,
    /// Circuit is half-open, allowing test requests.
    HalfOpen = 2,
}

impl From<u8> for CircuitState {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Open,
            2 => Self::HalfOpen,
            _ => Self::Closed,
        }
    }
}

/// Configuration for circuit breaker behavior.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit.
    pub failure_threshold: u64,
    /// Duration to keep circuit open before allowing test requests.
    pub reset_timeout: Duration,
    /// Number of successes in half-open state to close circuit.
    pub success_threshold: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 2,
        }
    }
}

/// Circuit breaker for protecting against external dependency failures.
///
/// Thread-safe implementation using atomic operations.
#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: AtomicU8,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: AtomicU64,
}

impl CircuitBreaker {
    #[allow(clippy::cast_possible_truncation)]
    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[allow(clippy::cast_possible_truncation)]
    const fn reset_timeout_millis(&self) -> u64 {
        self.config.reset_timeout.as_millis() as u64
    }

    /// Creates a new circuit breaker with the given configuration.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Atomic::new is not const fn
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: AtomicU64::new(0),
        }
    }

    /// Creates a circuit breaker with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Returns the current state of the circuit.
    #[must_use]
    pub fn state(&self) -> CircuitState {
        let raw = self.state.load(Ordering::SeqCst);
        CircuitState::from(raw)
    }

    /// Checks if a request should be allowed.
    ///
    /// Returns `Ok(())` if the request can proceed.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::CircuitOpen`] if the circuit is open and the
    /// reset timeout has not elapsed.
    pub fn check(&self) -> Result<()> {
        match self.state() {
            CircuitState::Closed | CircuitState::HalfOpen => Ok(()),
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                let last_failure = self.last_failure_time.load(Ordering::SeqCst);
                let now = Self::now_millis();
                let elapsed = now.saturating_sub(last_failure);

                let timeout_ms = self.reset_timeout_millis();
                if elapsed >= timeout_ms {
                    // Transition to half-open exactly once. Only the winning
                    // thread resets success_count, so concurrent checks cannot
                    // erase successes that have already been recorded.
                    match self.state.compare_exchange(
                        CircuitState::Open as u8,
                        CircuitState::HalfOpen as u8,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => {
                            self.success_count.store(0, Ordering::SeqCst);
                            Ok(())
                        }
                        Err(actual) => match CircuitState::from(actual) {
                            CircuitState::Closed | CircuitState::HalfOpen => Ok(()),
                            CircuitState::Open => Err(GraphError::CircuitOpen),
                        },
                    }
                } else {
                    Err(GraphError::CircuitOpen)
                }
            }
        }
    }

    /// Records a successful operation.
    pub fn record_success(&self) {
        match self.state() {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    // Close the circuit
                    self.state
                        .store(CircuitState::Closed as u8, Ordering::SeqCst);
                    self.failure_count.store(0, Ordering::SeqCst);
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Records a failed operation.
    pub fn record_failure(&self) {
        match self.state() {
            CircuitState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                self.last_failure_time
                    .store(Self::now_millis(), Ordering::SeqCst);
                if failures >= self.config.failure_threshold {
                    self.state.store(CircuitState::Open as u8, Ordering::SeqCst);
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state opens the circuit
                self.success_count.store(0, Ordering::SeqCst);
                self.failure_count.fetch_add(1, Ordering::SeqCst);
                self.last_failure_time
                    .store(Self::now_millis(), Ordering::SeqCst);
                self.state.store(CircuitState::Open as u8, Ordering::SeqCst);
            }
            CircuitState::Open => {}
        }
    }

    /// Resets the circuit breaker to closed state.
    pub fn reset(&self) {
        self.state
            .store(CircuitState::Closed as u8, Ordering::SeqCst);
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
    }

    /// Returns the current failure count.
    #[must_use]
    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::with_defaults();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check().is_ok());
    }

    #[test]
    fn opens_after_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(cb.check().is_err());
    }

    #[test]
    fn success_resets_failure_count() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn reset_closes_circuit() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.check().is_ok());
    }

    #[test]
    fn half_open_closes_on_success() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(0),
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_opens_on_failure() {
        let cb = CircuitBreaker::with_defaults();
        cb.state
            .store(CircuitState::HalfOpen as u8, Ordering::SeqCst);

        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn open_failures_do_not_extend_reset_timeout() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let cb = CircuitBreaker::new(config);

        cb.record_failure();
        let opened_at = cb.last_failure_time.load(Ordering::SeqCst);
        let failures = cb.failure_count();

        cb.record_failure();

        assert_eq!(cb.failure_count(), failures);
        assert_eq!(cb.last_failure_time.load(Ordering::SeqCst), opened_at);
    }

    #[test]
    fn circuit_open_error_is_retryable() {
        let err = GraphError::CircuitOpen;
        assert!(err.is_retryable());
        assert_eq!(err.code(), "FG-SYS-E001");
    }
}
