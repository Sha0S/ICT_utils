# Utilities for Keysight ICTs:

## libraries:
- config: Shared classes for interpreting the configuration files used by the binaries.
- log_file: Interpreting Keysight log files, and processing their data.
- auth: Library for the local users of the traceability_server

## binaries:
- analysis: Graphical representation of production, yields, graphs for the tests, etc.
- query: Get ICT (and CCL) results from a SQL database.
- log_reader: Basic interpreter for logfiles. 
- traceability_client and traceability_server: Improved MES system, with multiple user levels.

## deprecated:
- traceabilty: Basic MES implementation, was replaced with traceability_client + server.
- aoi_uploader: Experimental program to interpret xml result files from AOI/AXI machines, and upload them to SQL.
