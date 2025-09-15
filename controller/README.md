# controller

A controller that syncs between a kubernetes cluster and a set of computercraft clusters.

## High-level design

A cluster consists of a set of computers, with a subset of computers designated as gateway nodes. All computers are self-registering, meaning they create themselves in the kubernetes cluster.

The cluster must first be created out of band. After this, each computer will be able to register itself under the specified cluster. 
