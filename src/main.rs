use std::error::Error;

use octocrab::{models, params};

#[tokio::main]
async fn main() {
    run().await.unwrap();
}

pub async fn run() -> Result<(), Box<dyn Error>> {
    let octocrab = octocrab::instance();
    // Returns the first page of all issues.
    let mut page = octocrab
        .issues("ddnet", "ddnet")
        .list()
        .state(params::State::All)
        .per_page(50)
        .send()
        .await?;

    // Go through every page of issues. Warning: There's no rate limiting so
    // be careful.
    loop {
        for issue in &page {
            println!("{}", issue.title);
        }
        page = match octocrab
            .get_page::<models::issues::Issue>(&page.next)
            .await?
        {
            Some(next_page) => next_page,
            None => break,
        }
    }
    Ok(())
}
