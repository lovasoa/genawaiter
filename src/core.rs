use crate::{ops::GeneratorState, waker};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub enum Next<Y, R> {
    Empty,
    Yield(Y),
    Resume(R),
}

pub fn advance<Y, R, F: Future>(
    future: Pin<&mut F>,
    airlock: &impl Airlock<Yield = Y, Resume = R>,
) -> GeneratorState<Y, F::Output> {
    let waker = waker::create();
    let mut cx = Context::from_waker(&waker);

    match future.poll(&mut cx) {
        Poll::Pending => {
            let value = airlock.replace(Next::Empty);
            match value {
                Next::Empty => unreachable!(),
                Next::Yield(y) => GeneratorState::Yielded(y),
                Next::Resume(_) => {
                    #[cfg(debug_assertions)]
                    panic!(
                        "A generator was awaited without first yielding a value. \
                         Inside a generator, do not await any futures other than the \
                         one returned by `Co::yield_`.",
                    );

                    #[cfg(not(debug_assertions))]
                    panic!("invalid await");
                }
            }
        }
        Poll::Ready(value) => GeneratorState::Complete(value),
    }
}

pub trait Airlock {
    type Yield;
    type Resume;

    fn peek(&self) -> Next<(), ()>;

    fn replace(
        &self,
        next: Next<Self::Yield, Self::Resume>,
    ) -> Next<Self::Yield, Self::Resume>;
}

pub struct Co<A: Airlock> {
    airlock: A,
}

impl<A: Airlock> Co<A> {
    pub(crate) fn new(airlock: A) -> Self {
        Self { airlock }
    }

    /// Yields a value from the generator.
    ///
    /// The caller should immediately `await` the result of this function.
    ///
    /// _See the module-level docs for examples._
    pub fn yield_(&self, value: A::Yield) -> impl Future<Output = A::Resume> + '_ {
        #[cfg(debug_assertions)]
        match self.airlock.peek() {
            Next::Yield(()) => {
                panic!(
                    "Multiple values were yielded without an intervening await. Make \
                     sure to immediately await the result of `Co::yield_`."
                );
            }
            Next::Empty | Next::Resume(()) => {}
        }

        self.airlock.replace(Next::Yield(value));
        Barrier {
            airlock: &self.airlock,
        }
    }
}

struct Barrier<'a, A: Airlock> {
    airlock: &'a A,
}

impl<'a, A: Airlock> Future for Barrier<'a, A> {
    type Output = A::Resume;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.airlock.peek() {
            Next::Empty => unreachable!(),
            Next::Yield(_) => Poll::Pending,
            Next::Resume(_) => {
                let next = self.airlock.replace(Next::Empty);
                match next {
                    Next::Empty => unreachable!(),
                    Next::Yield(_) => unreachable!(),
                    Next::Resume(arg) => Poll::Ready(arg),
                }
            }
        }
    }
}
