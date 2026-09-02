# Archer Bug Bounty Program

## Bug Bounty Overview

This bug bounty program is specifically for Archer’s smart contract code, the program deployed at `Archer8kgiavM61GyusMzaaS2ft5sALtNsD1HxkUPMhy`. The program source is not public; this repository is the public artifact for it — the IDL, account layouts, instruction builders and quoting math.

The bug bounty program is administered by Ballista Technologies LLC

## Security Classifications and Bounty Amounts

<table>
  <tbody>
    <tr>
      <th>Severity</th>
      <th>Description</th>
      <th>Bounty</th>
    </tr>
    <tr>
      <td>Critical</td>
      <td>
        <ul>
          <li>Direct theft of funds</li>
          <li>Permanent freezing of funds</li>
          <li>Vulnerabilities that lead to protocol insolvency</li>
        </ul>
      </td>
      <td>Up to $20,000</td>
    </tr>
    <tr>
      <td>High</td>
      <td>
        <ul>
          <li>Temporary freezing of user funds</li>
        </ul>
      </td>
      <td>Up to $15,000</td>
    </tr>
    <tr>
      <td>Medium</td>
      <td>
        <ul>
          <li>Slow-drip drainage of funds that’s profitable to the attacker</li>
          <li>Orders getting filled at a worse price than specified</li>
          <li>Theft of rent</li>
          <li>Loss of data</li>
          <li>Unintended reuse of sequence numbers</li>
        </ul>
      </td>
      <td>Up to $10,000</td>
    </tr>
    <tr>
      <td>Low</td>
      <td>
        <ul>
          <li>Griefing (no profit for the attacker, but damage to the protocol or its users)</li>
          <li>Temporary denial of service</li>
        </ul>
      </td>
      <td>Up to $5,000</td>
    </tr>
  </tbody>
</table>

Bugs in `archer-market-maker` and other code outside of the smart contract will be assessed on a case-by-case basis.

## Report Submission

Please email dhrumil@archer.exchange with a detailed description of the attack vector. We will reach back out within 24 hours with additional questions or next steps on the bug bounty.

## Scope

The following components are explicitly out of scope for the bounty program.

- Vulnerabilities that the reporter has already exploited themselves, leading to damage
- Any UI bugs
- Bugs in the core Solana runtime (please submit these to [Solana’s bug bounty program](https://github.com/solana-labs/solana/security/policy))
- Bugs in the Sokoban library that do not affect the red-black tree
- Vulnerabilities that require a validator to execute them
- Vulnerabilities requiring access to privileged keys/credentials
- MEV vectors the team is already aware of