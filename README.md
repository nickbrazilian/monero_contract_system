#   Monero Contract System

## Project Goal
This project aims to create an open-source web application that allows users to host their own arbitration/escrow platform. Users can specify requirements (e.g., "I want a website built like this in 3 days"), deposit funds into escrow, release funds upon task completion, or contact the website administrator for arbitration if disputes arise.

---

## How to Run
1. Pull the Docker image:
   ```bash
   docker pull nickbrazilian/xmr-contracts:MVP

2. Run the container:
        ``````bash
        docker run -d \
        --name xmr-contracts \
        -p 8080:8080 \
        -p 18088:18088 \
        -v xmr-contracts-data:/app/data \
        -e TZ=UTC \
        nickbrazilian/xmr-contracts:latest

# **VIDEO:** [Monero Contract System MVP](https://nicolasbianconi.com)

## FAQ
1. **Do I have to trust the website administrator?**  
   Yes, you must trust the website administrator to act fairly in arbitration decisions.

2. **Can the system be decentralized without trusting a website owner?**  
   Yes! A future solution (planned in Rust) will mimic Bisq (a decentralized Bitcoin exchange), using AI for arbitration and a fallback "blockchain judicial system" for disputes. This MVP exists as a quick resume project, but the decentralized version is a long-term goal.

📧 **Contact Me**  
Reach out at [nicolas@nicolasbianconi.com](mailto:nicolas@nicolasbianconi.com) to discuss collaboration or project evolution!
