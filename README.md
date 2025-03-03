#   Monero Contract System

## Project Goal
This project aims to create an open-source web application that allows users to host their own arbitration/escrow platform. Users can specify requirements (e.g., "I want a website built like this in 3 days"), deposit funds into escrow, release funds upon task completion, or contact the website administrator for arbitration if disputes arise.

## How to Run
1. Pull the Docker image:
   ```bash
   docker pull nickbrazilian/xmr-contracts:MVP

2. Run the container:
   ```bash
   docker run -d \
   --name xmr-contracts \
   -p 8080:8080 \
   -p 18088:18088 \
   -v xmr-contracts-data:/app/data \
   -e TZ=UTC \
   nickbrazilian/xmr-contracts:MVP

# **WATCH THE VIDEO INSTEAD:** [Monero Contract System MVP on odysee](https://odysee.com/@nickbrazilian:b/monero-contracts-system:5)

## FAQ
1. **Do I have to trust the website administrator?**  
   Yes, you must trust the website administrator to act fairly in arbitration decisions.

2. **Can there be a decentralized autonomous system the does not require us to trust the webiste owner?**  
   Yes! A future solution (in Rust) will mimic Bisq (a decentralized Bitcoin exchange), using AI for disputes.
   I am not working on this project right now because i wanted to have some good looking project with MVP on my CV immediatly.

## 📧 **Contact Me**  
Reach out at [nicolas@nicolasbianconi.com](mailto:nicolas@nicolasbianconi.com) to discuss collaboration or project evolution!
