use std::future::Future;
use std::future::poll_fn;
use std::pin::Pin;
use std::task::Poll;

pub(super) async fn join_all<F>(futures: Vec<F>) -> Vec<F::Output>
where
    F: Future + Send,
    F::Output: Send,
{
    let mut futures = futures
        .into_iter()
        .map(|future| Some(Box::pin(future)))
        .collect::<Vec<Option<Pin<Box<F>>>>>();
    let mut outputs = std::iter::repeat_with(|| None)
        .take(futures.len())
        .collect::<Vec<Option<F::Output>>>();
    poll_fn(|context| {
        let mut pending = false;
        for (future, output) in futures.iter_mut().zip(&mut outputs) {
            let Some(running) = future else {
                continue;
            };
            match running.as_mut().poll(context) {
                Poll::Ready(result) => {
                    *output = Some(result);
                    *future = None;
                }
                Poll::Pending => pending = true,
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}
