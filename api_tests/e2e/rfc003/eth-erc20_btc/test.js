const chai = require("chai");
chai.use(require("chai-http"));
const wallet = require("../../../lib/wallet.js");
const Web3 = require("web3");
const ethereum = require("../../../lib/ethereum.js");
const bitcoin = require("../../../lib/bitcoin.js");
const actor = require("../../../lib/actor.js");
const should = chai.should();
const logger = global.harness.logger;
const bitcoin_rpc_client = bitcoin.create_client();

const toby_wallet = wallet.create("toby");

const toby_initial_eth = "10";
const alice_initial_eth = "5";
const alice_initial_erc20 = Web3.utils.toWei("10000", "ether");

const alice = actor.create("alice");
const bob = actor.create("bob");

const alice_final_address = "mzNFGtxdTSTJ1Lh6fq5N5oUgbhwA7Nm7cAx";
const bob_final_address = "0x00a329c0648769a73afac7f9381e08fb43dbea72";
const bob_comit_node_address = bob.config.comit.comit_listen;
const alpha_asset_amount = BigInt(Web3.utils.toWei("5000", "ether"));

const beta_asset_amount = 100000000;
const beta_max_fee = 5000; // Max 5000 satoshis fee
const alpha_expiry = new Date("2080-06-11T23:00:00Z").getTime() / 1000;
const beta_expiry = new Date("2080-06-11T13:00:00Z").getTime() / 1000;

describe("RFC003: ERC20 for Bitcoin", () => {
    let token_contract_address;
    before(async function() {
        this.timeout(5000);
        await bitcoin.btc_activate_segwit();
        await toby_wallet.eth().fund(toby_initial_eth);
        await alice.wallet.eth().fund(alice_initial_eth);
        await bob.wallet.btc().fund(10);
        await bob.wallet.eth().fund(1);
        let receipt = await toby_wallet.eth().deploy_erc20_token_contract();
        token_contract_address = receipt.contractAddress;

        await bitcoin.btc_import_address(alice_final_address); // Watch only import
        await bitcoin.btc_generate();
    });

    it(alice_initial_erc20 + " tokens were minted to Alice", async function() {
        let alice_wallet_address = alice.wallet.eth().address();

        let receipt = await ethereum.mint_erc20_tokens(
            toby_wallet,
            token_contract_address,
            alice_wallet_address,
            alice_initial_erc20
        );

        receipt.status.should.equal(true);

        let erc20_balance = await ethereum.erc20_balance(
            alice_wallet_address,
            token_contract_address
        );
        erc20_balance.toString().should.equal(alice_initial_erc20);
    });

    let swap_location;
    let alice_swap_href;

    it("[Alice] Should be able to make a swap request via HTTP api", async () => {
        let res = await chai
            .request(alice.comit_node_url())
            .post("/swaps/rfc003")
            .send({
                alpha_ledger: {
                    name: "Ethereum",
                    network: "regtest",
                },
                beta_ledger: {
                    name: "Bitcoin",
                    network: "regtest",
                },
                alpha_asset: {
                    name: "ERC20",
                    quantity: alpha_asset_amount.toString(),
                    token_contract: token_contract_address,
                },
                beta_asset: {
                    name: "Bitcoin",
                    quantity: beta_asset_amount.toString(),
                },
                alpha_ledger_refund_identity: bob_final_address,
                beta_ledger_redeem_identity: null,
                alpha_expiry: alpha_expiry,
                beta_expiry: beta_expiry,
                peer: bob_comit_node_address,
            });

        res.should.have.status(201);
        swap_location = res.headers.location;
        logger.info("Alice created a new swap at %s", swap_location);
        swap_location.should.be.a("string");
        alice_swap_href = swap_location;
    });

    it("[Alice] Should be in Start state after sending the swap request to Bob", async function() {
        await alice.poll_comit_node_until(chai, alice_swap_href, "Start");
    });

    let bob_swap_href;

    it("[Bob] Shows the Swap as Start in /swaps", async () => {
        let res = await chai.request(bob.comit_node_url()).get("/swaps");

        let embedded = res.body._embedded;
        let swap_embedded = embedded.swaps[0];
        swap_embedded.protocol.should.equal("rfc003");
        swap_embedded.state.should.equal("Start");
        let swap_link = swap_embedded._links;
        swap_link.should.be.a("object");
        bob_swap_href = swap_link.self.href;
        bob_swap_href.should.be.a("string");

        logger.info("Bob discovered a new swap at %s", bob_swap_href);
    });

    let bob_accept_href;

    it("[Bob] Can get the accept action", async () => {
        let res = await chai.request(bob.comit_node_url()).get(bob_swap_href);
        res.should.have.status(200);
        res.body.state.should.equal("Start");
        res.body._links.accept.href.should.be.a("string");
        bob_accept_href = res.body._links.accept.href;
    });

    it("[Bob] Can execute the accept action", async () => {
        let bob_response = {
            beta_ledger_refund_identity: bob.wallet.eth().address(),
            alpha_ledger_redeem_identity: bob_final_address,
        };

        logger.info(
            "Bob is accepting the swap via %s with the following parameters",
            bob_accept_href,
            bob_response
        );

        let accept_res = await chai
            .request(bob.comit_node_url())
            .post(bob_accept_href)
            .send(bob_response);

        accept_res.should.have.status(200);
    });

    it("[Bob] Should be in the Accepted State after accepting", async () => {
        let res = await chai.request(bob.comit_node_url()).get(bob_swap_href);
        res.should.have.status(200);
        res.body.state.should.equal("Accepted");
    });

    let alice_deploy_href;

    it("[Alice] Can get the HTLC deploy action", async () => {
        let res = await chai
            .request(alice.comit_node_url())
            .get(alice_swap_href);
        res.should.have.status(200);
        res.body.state.should.equal("Accepted");
        let links = res.body._links;
        links.should.have.property("deploy");
        alice_deploy_href = links.deploy.href;
    });

    let alice_deploy_action;

    it("[Alice] Can get the deploy action from the ‘deploy’ link", async () => {
        let res = await chai
            .request(alice.comit_node_url())
            .get(alice_deploy_href);
        res.should.have.status(200);
        alice_deploy_action = res.body;

        logger.info(
            "Alice retrieved the following deploy parameters",
            alice_deploy_action
        );
    });

    it("[Alice] Can execute the deploy action", async () => {
        alice_deploy_action.payload.should.include.all.keys(
            "data",
            "amount",
            "gas_limit",
            "network"
        );
        alice_deploy_action.payload.amount.should.equal("0");
        await alice.do(alice_deploy_action);
    });

    it("[Bob] Should be in AlphaDeployed state after Alice executes the deploy action", async function() {
        this.timeout(10000);
        await bob.poll_comit_node_until(chai, bob_swap_href, "AlphaDeployed");
    });

    let alice_fund_href;

    it("[Alice] Should be in AlphaDeployed state after executing the deploy action", async function() {
        this.timeout(10000);
        let swap = await alice.poll_comit_node_until(
            chai,
            alice_swap_href,
            "AlphaDeployed"
        );
        let links = swap._links;
        links.should.have.property("fund");
        alice_fund_href = links.fund.href;
    });

    let alice_fund_action;

    it("[Alice] Can get the fund action from the ‘fund’ link", async () => {
        let res = await chai
            .request(alice.comit_node_url())
            .get(alice_fund_href);
        res.should.have.status(200);
        alice_fund_action = res.body;

        logger.info(
            "Alice retrieved the following funding parameters",
            alice_fund_action
        );
    });

    it("[Alice] Can execute the fund action", async () => {
        alice_fund_action.payload.should.include.all.keys(
            "contract_address",
            "data",
            "amount",
            "gas_limit",
            "network"
        );
        let receipt = await alice.do(alice_fund_action);
        receipt.status.should.equal(true);
    });

    it("[Alice] Should be in AlphaFunded state after executing the funding action", async function() {
        this.timeout(10000);
        await alice.poll_comit_node_until(chai, alice_swap_href, "AlphaFunded");
    });

    let bob_funding_href;

    it("[Bob] Should be in AlphaFunded state after Alice executes the funding action", async function() {
        this.timeout(10000);
        let swap = await bob.poll_comit_node_until(
            chai,
            bob_swap_href,
            "AlphaFunded"
        );
        swap.should.have.property("_links");
        swap._links.should.have.property("fund");
        bob_funding_href = swap._links.fund.href;
    });

    let bob_funding_action;

    it("[Bob] Can get the funding action from the ‘fund’ link", async () => {
        let res = await chai
            .request(bob.comit_node_url())
            .get(bob_funding_href);
        res.should.have.status(200);
        bob_funding_action = res.body;

        logger.info(
            "Bob retrieved the following funding parameters",
            bob_funding_action
        );
    });

    it("[Bob] Can execute the funding action", async () => {
        bob_funding_action.payload.should.include.all.keys(
            "to",
            "amount",
            "network"
        );
        await bob.do(bob_funding_action);
    });

    let alice_redeem_href;

    it("[Alice] Should be in BothFunded state after Bob executes the funding action", async function() {
        this.timeout(10000);
        let swap = await alice.poll_comit_node_until(
            chai,
            alice_swap_href,
            "BothFunded"
        );
        swap.should.have.property("_links");
        swap._links.should.have.property("redeem");
        alice_redeem_href = swap._links.redeem.href;
    });

    it("[Bob] Should be in BothFunded state after executing the funding action", async function() {
        this.timeout(10000);
        await bob.poll_comit_node_until(chai, bob_swap_href, "BothFunded");
    });

    let alice_redeem_action;

    it("[Alice] Can get the redeem action from the ‘redeem’ link", async () => {
        let res = await chai
            .request(alice.comit_node_url())
            .get(
                alice_redeem_href +
                    "?address=" +
                    alice_final_address +
                    "&fee_per_byte=20"
            );
        res.should.have.status(200);
        alice_redeem_action = res.body;

        logger.info(
            "Alice retrieved the following redeem parameters",
            alice_redeem_action
        );
    });

    let alice_btc_balance_before;

    it("[Alice] Can execute the redeem action", async function() {
        alice_redeem_action.payload.should.include.all.keys("hex", "network");
        alice_btc_balance_before = await bitcoin.btc_balance(
            alice_final_address
        );
        await alice.do(alice_redeem_action);
        await bitcoin.btc_generate();
    });

    it("[Alice] Should be in AlphaFundedBetaRedeemed state after executing the redeem action", async function() {
        this.timeout(10000);
        await alice.poll_comit_node_until(
            chai,
            alice_swap_href,
            "AlphaFundedBetaRedeemed"
        );
    });

    it("[Alice] Should have received the beta asset after the redeem", async function() {
        let alice_btc_balance_after = await bitcoin.btc_balance(
            alice_final_address
        );

        const alice_btc_balance_expected =
            alice_btc_balance_before + beta_asset_amount - beta_max_fee;
        alice_btc_balance_after.should.be.at.least(alice_btc_balance_expected);
    });

    let bob_redeem_href;

    it("[Bob] Should be in AlphaFundedBetaRedeemed state after Alice executes the redeem action", async function() {
        this.timeout(10000);
        let swap = await bob.poll_comit_node_until(
            chai,
            bob_swap_href,
            "AlphaFundedBetaRedeemed"
        );
        swap.should.have.property("_links");
        swap._links.should.have.property("redeem");
        bob_redeem_href = swap._links.redeem.href;
    });

    let bob_redeem_action;

    it("[Bob] Can get the redeem action from the ‘redeem’ link", async () => {
        let res = await chai.request(bob.comit_node_url()).get(bob_redeem_href);
        res.should.have.status(200);
        bob_redeem_action = res.body;

        logger.info(
            "Bob retrieved the following redeem parameters",
            bob_redeem_action
        );
    });

    let bob_erc20_balance_before;

    it("[Bob] Can execute the redeem action", async function() {
        bob_redeem_action.payload.should.include.all.keys(
            "contract_address",
            "data",
            "amount",
            "gas_limit",
            "network"
        );
        bob_erc20_balance_before = await ethereum.erc20_balance(
            bob_final_address,
            token_contract_address
        );
        await bob.do(bob_redeem_action);
    });

    it("[Alice] Should be in BothRedeemed state after Bob executes the redeem action", async function() {
        this.timeout(10000);
        await alice.poll_comit_node_until(
            chai,
            alice_swap_href,
            "BothRedeemed"
        );
    });

    it("[Bob] Should be in BothRedeemed state after executing the redeem action", async function() {
        this.timeout(10000);
        await bob.poll_comit_node_until(chai, bob_swap_href, "BothRedeemed");
    });

    it("[Bob] Should have received the alpha asset after the redeem", async function() {
        let bob_erc20_balance_after = await ethereum.erc20_balance(
            bob_final_address,
            token_contract_address
        );

        let bob_erc20_balance_expected =
            bob_erc20_balance_before + alpha_asset_amount;

        bob_erc20_balance_after
            .toString()
            .should.be.equal(bob_erc20_balance_expected.toString());
    });
});
