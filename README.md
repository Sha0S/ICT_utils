# Utilities for SMT:

## libraries:
- config: Shared classes for interpreting the configuration files used by the binaries.
- log_file: Interpreting ICT and FCT logfiles, and processing their data.
- aoi_log: Interpreting AOI XMLs.
- auth: Library for the local users of the traceability_server

## binaries:
- analysis: Graphical representation of production, yields, graphs for the tests, etc.
- aoi_uploader: Processes and uploads XMLs from AOIs into SQL.
- auth_manager: Manages users for traceability_server.
- log_reader: Basic interpreter for ICT/FCT logfiles. 
- query: Get SMT results from a SQL database.
- smt_yield: WIP. Query yield and failures from SQL. 
- traceability_client and traceability_server: MES for ICTs.
